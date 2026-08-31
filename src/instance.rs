use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub enum AcquireResult {
    Owner(Instance),
    StoppedExisting,
}

pub struct Instance {
    listener: UnixListener,
    path: PathBuf,
    identity: (u64, u64),
}

pub struct PendingStop {
    stream: UnixStream,
}

impl PendingStop {
    pub fn acknowledge(mut self) -> io::Result<()> {
        self.stream.write_all(b"OK\n")
    }
}

impl Instance {
    pub fn acquire(runtime_dir: &Path) -> io::Result<AcquireResult> {
        let _recovery_lock = acquire_recovery_lock(runtime_dir)?;
        let path = runtime_dir.join("wlapse.sock");
        match bind_owner(path.clone()) {
            Ok(owner) => Ok(AcquireResult::Owner(owner)),
            Err(error) if error.kind() == ErrorKind::AddrInUse => match stop_existing(&path) {
                Ok(()) => Ok(AcquireResult::StoppedExisting),
                Err(connect_error)
                    if matches!(
                        connect_error.kind(),
                        ErrorKind::ConnectionRefused | ErrorKind::NotFound
                    ) =>
                {
                    match fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(remove_error) if remove_error.kind() == ErrorKind::NotFound => {}
                        Err(remove_error) => return Err(remove_error),
                    }

                    match bind_owner(path.clone()) {
                        Ok(owner) => Ok(AcquireResult::Owner(owner)),
                        Err(retry_error) if retry_error.kind() == ErrorKind::AddrInUse => {
                            stop_existing(&path)?;
                            Ok(AcquireResult::StoppedExisting)
                        }
                        Err(retry_error) => Err(retry_error),
                    }
                }
                Err(connect_error) => Err(connect_error),
            },
            Err(error) => Err(error),
        }
    }

    pub fn accept_stop(&self) -> io::Result<Option<PendingStop>> {
        let (mut stream, _) = match self.listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(None),
            Err(error) => return Err(error),
        };
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;

        let mut command = [0_u8; 5];
        stream.read_exact(&mut command)?;
        if !is_stop_command(&command) {
            return Ok(None);
        }

        Ok(Some(PendingStop { stream }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn listener(&self) -> &UnixListener {
        &self.listener
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if let Some(Ok(_recovery_lock)) = self.path.parent().map(acquire_recovery_lock) {
            remove_if_owned(&self.path, self.identity);
        }
    }
}

fn bind_owner(path: PathBuf) -> io::Result<Instance> {
    let listener = UnixListener::bind(&path)?;
    let identity = socket_identity(&path)?;
    if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
        remove_if_owned(&path, identity);
        return Err(error);
    }
    if let Err(error) = listener.set_nonblocking(true) {
        remove_if_owned(&path, identity);
        return Err(error);
    }
    Ok(Instance {
        listener,
        path,
        identity,
    })
}

fn acquire_recovery_lock(runtime_dir: &Path) -> io::Result<UnixListener> {
    let metadata = fs::metadata(runtime_dir)?;
    let name = format!("wlapse-acquire-{:x}-{:x}", metadata.dev(), metadata.ino());
    let address = SocketAddr::from_abstract_name(name)?;
    let deadline = Instant::now() + Duration::from_secs(2);

    loop {
        match UnixListener::bind_addr(&address) {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == ErrorKind::AddrInUse && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) if error.kind() == ErrorKind::AddrInUse => {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    "timed out waiting for instance acquisition",
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

fn socket_identity(path: &Path) -> io::Result<(u64, u64)> {
    let metadata = fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

fn remove_if_owned(path: &Path, identity: (u64, u64)) {
    if socket_identity(path).is_ok_and(|current| current == identity) {
        let _ = fs::remove_file(path);
    }
}

fn stop_existing(path: &Path) -> io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    stream.write_all(b"STOP\n")?;

    let mut response = [0_u8; 3];
    stream.read_exact(&mut response)?;
    if response == *b"OK\n" {
        Ok(())
    } else {
        Err(io::Error::new(
            ErrorKind::InvalidData,
            "wlapse owner returned an invalid response",
        ))
    }
}

pub fn is_stop_command(command: &[u8]) -> bool {
    command == b"STOP\n"
}

#[cfg(test)]
mod tests {
    use super::{AcquireResult, Instance, is_stop_command};
    use std::fs;
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{SocketAddr, UnixListener};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_runtime_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "wlapse-test-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create test runtime dir");
        path
    }

    #[test]
    fn accepts_only_exact_stop_command() {
        assert!(is_stop_command(b"STOP\n"));
        assert!(!is_stop_command(b"STOP"));
        assert!(!is_stop_command(b"stop\n"));
        assert!(!is_stop_command(b"STOP\nextra"));
    }

    #[test]
    fn first_instance_owns_a_private_socket() {
        let runtime_dir = temp_runtime_dir();
        let acquired = Instance::acquire(&runtime_dir).expect("acquire instance");
        let AcquireResult::Owner(instance) = acquired else {
            panic!("first instance did not become owner");
        };

        let metadata = fs::metadata(instance.path()).expect("socket metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);

        drop(instance);
        assert!(!runtime_dir.join("wlapse.sock").exists());
        fs::remove_dir(runtime_dir).expect("remove test runtime dir");
    }

    #[test]
    fn second_instance_stops_the_owner_and_waits_for_ack() {
        let runtime_dir = temp_runtime_dir();
        let AcquireResult::Owner(owner) = Instance::acquire(&runtime_dir).expect("acquire owner")
        else {
            panic!("first instance did not become owner");
        };

        let owner_thread = std::thread::spawn(move || {
            loop {
                if let Some(stop) = owner.accept_stop().expect("accept stop") {
                    stop.acknowledge().expect("acknowledge stop");
                    break;
                }
                std::thread::yield_now();
            }
        });

        let result = Instance::acquire(&runtime_dir).expect("stop owner");
        assert!(matches!(result, AcquireResult::StoppedExisting));
        owner_thread.join().expect("join owner");

        fs::remove_dir(runtime_dir).expect("remove test runtime dir");
    }

    #[test]
    fn owner_drop_does_not_remove_a_replacement_socket() {
        let runtime_dir = temp_runtime_dir();
        let socket_path = runtime_dir.join("wlapse.sock");
        let moved_path = runtime_dir.join("old-wlapse.sock");
        let AcquireResult::Owner(owner) =
            Instance::acquire(&runtime_dir).expect("acquire original owner")
        else {
            panic!("first instance did not become owner");
        };

        fs::rename(&socket_path, &moved_path).expect("move original socket");
        let replacement =
            std::os::unix::net::UnixListener::bind(&socket_path).expect("bind replacement");

        drop(owner);
        assert!(
            socket_path.exists(),
            "dropping an old owner removed a newer owner's socket"
        );

        drop(replacement);
        fs::remove_file(socket_path).expect("remove replacement socket");
        fs::remove_file(moved_path).expect("remove moved socket");
        fs::remove_dir(runtime_dir).expect("remove test runtime dir");
    }

    #[test]
    fn acquisition_waits_for_the_runtime_recovery_lock() {
        let runtime_dir = temp_runtime_dir();
        let metadata = fs::metadata(&runtime_dir).expect("runtime metadata");
        let lock_name = format!("wlapse-acquire-{:x}-{:x}", metadata.dev(), metadata.ino());
        let lock_address =
            SocketAddr::from_abstract_name(lock_name).expect("abstract lock address");
        let lock = UnixListener::bind_addr(&lock_address).expect("hold recovery lock");

        let (sender, receiver) = mpsc::channel();
        let thread_runtime = runtime_dir.clone();
        let contender = std::thread::spawn(move || {
            sender
                .send(Instance::acquire(&thread_runtime))
                .expect("send acquire result");
        });

        assert!(
            matches!(
                receiver.recv_timeout(Duration::from_millis(25)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "acquisition entered stale recovery while another process held its lock"
        );

        drop(lock);
        let acquired = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("acquisition did not resume after lock release")
            .expect("acquire owner");
        assert!(matches!(acquired, AcquireResult::Owner(_)));
        drop(acquired);
        contender.join().expect("join contender");
        fs::remove_dir(runtime_dir).expect("remove test runtime dir");
    }

    #[test]
    fn recovers_a_stale_socket_file() {
        let runtime_dir = temp_runtime_dir();
        let socket_path = runtime_dir.join("wlapse.sock");
        let stale = std::os::unix::net::UnixListener::bind(&socket_path).expect("bind stale");
        drop(stale);

        let result = Instance::acquire(&runtime_dir).expect("recover stale socket");
        assert!(matches!(result, AcquireResult::Owner(_)));
        drop(result);

        fs::remove_dir(runtime_dir).expect("remove test runtime dir");
    }
}
