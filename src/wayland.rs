use crate::clock::format_elapsed;
use crate::config::Colors;
use crate::instance::Instance;
use crate::placement::{Placement, Position};
use crate::render::{HEIGHT, WIDTH, render_timer};
use memmap2::{MmapMut, MmapOptions};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use std::error::Error;
use std::fs::File;
use std::os::fd::AsFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::wp::relative_pointer::zv1::client::{
    zwp_relative_pointer_manager_v1, zwp_relative_pointer_v1,
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

const BUFFER_BYTES: usize = WIDTH * HEIGHT * 4;
const BTN_LEFT: u32 = 0x110;

pub struct WaylandApp {
    connection: Connection,
    event_queue: EventQueue<State>,
    state: State,
}

impl WaylandApp {
    pub fn connect(placement: Placement, colors: Colors) -> Result<Self, Box<dyn Error>> {
        let connection = Connection::connect_to_env()?;
        let mut event_queue = connection.new_event_queue();
        let qh = event_queue.handle();
        connection.display().get_registry(&qh, ());

        let mut state = State::new(placement, colors);
        event_queue.roundtrip(&mut state)?;
        state.initialize(&qh)?;
        connection.flush()?;

        Ok(Self {
            connection,
            event_queue,
            state,
        })
    }

    pub fn event_queue(&mut self) -> &mut EventQueue<State> {
        &mut self.event_queue
    }

    pub fn state(&mut self) -> &mut State {
        &mut self.state
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn is_running(&self) -> bool {
        self.state.running
    }

    pub fn run(
        mut self,
        instance: &Instance,
        terminate: Arc<AtomicBool>,
    ) -> Result<(), Box<dyn Error>> {
        let run_result = self.run_loop(instance, &terminate);
        let shutdown_result = self.shutdown();
        run_result?;
        shutdown_result
    }

    fn run_loop(
        &mut self,
        instance: &Instance,
        terminate: &AtomicBool,
    ) -> Result<(), Box<dyn Error>> {
        let timeout = Timespec {
            tv_sec: 0,
            tv_nsec: 250_000_000,
        };

        while self.state.running && !terminate.load(Ordering::Relaxed) {
            self.event_queue.flush()?;
            self.event_queue.dispatch_pending(&mut self.state)?;
            self.state.check_error()?;

            let Some(read_guard) = self.event_queue.prepare_read() else {
                continue;
            };
            let (wayland_ready, stop_ready) = {
                let mut fds = [
                    PollFd::from_borrowed_fd(read_guard.connection_fd(), PollFlags::IN),
                    PollFd::new(instance.listener(), PollFlags::IN),
                ];
                match poll(&mut fds, Some(&timeout)) {
                    Ok(_) => {}
                    Err(rustix::io::Errno::INTR) => {
                        drop(read_guard);
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                }
                (
                    fds[0]
                        .revents()
                        .intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP),
                    fds[1].revents().contains(PollFlags::IN),
                )
            };

            if wayland_ready {
                read_guard.read()?;
            } else {
                drop(read_guard);
            }
            self.event_queue.dispatch_pending(&mut self.state)?;
            self.state.check_error()?;

            if stop_ready {
                let Some(stop) = instance.accept_stop()? else {
                    continue;
                };
                let shutdown_result = self.shutdown();
                let acknowledge_result = stop.acknowledge();
                acknowledge_result?;
                shutdown_result?;
                return Ok(());
            }
        }

        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        let placement_result = self.state.shutdown();
        let flush_result = self.connection.flush();
        placement_result?;
        flush_result?;
        Ok(())
    }
}

pub struct State {
    running: bool,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    relative_pointer_manager: Option<zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1>,
    relative_pointer: Option<zwp_relative_pointer_v1::ZwpRelativePointerV1>,
    surface: Option<wl_surface::WlSurface>,
    layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    buffers: Vec<BufferSlot>,
    configured: bool,
    frame_pending: bool,
    started_at: Option<Instant>,
    colors: Colors,
    placement: Placement,
    fatal_error: Option<String>,
}

struct BufferSlot {
    proxy: wl_buffer::WlBuffer,
    memory: MmapMut,
    busy: bool,
}

impl State {
    fn new(placement: Placement, colors: Colors) -> Self {
        Self {
            running: false,
            compositor: None,
            shm: None,
            layer_shell: None,
            seat: None,
            pointer: None,
            relative_pointer_manager: None,
            relative_pointer: None,
            surface: None,
            layer_surface: None,
            buffers: Vec::new(),
            configured: false,
            frame_pending: false,
            started_at: None,
            colors,
            placement,
            fatal_error: None,
        }
    }

    fn initialize(&mut self, qh: &QueueHandle<Self>) -> Result<(), Box<dyn Error>> {
        let compositor = self
            .compositor
            .as_ref()
            .ok_or("compositor does not provide wl_compositor")?;
        let shm = self
            .shm
            .as_ref()
            .ok_or("compositor does not provide wl_shm")?;
        let layer_shell = self
            .layer_shell
            .as_ref()
            .ok_or("compositor does not support wlr-layer-shell")?;
        self.relative_pointer_manager
            .as_ref()
            .ok_or("compositor does not support relative-pointer-v1")?;

        let surface = compositor.create_surface(qh, ());
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Overlay,
            "wlapse".to_owned(),
            qh,
            (),
        );
        layer_surface.set_size(WIDTH as u32, HEIGHT as u32);
        layer_surface
            .set_anchor(zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left);
        let position = self.placement.position();
        layer_surface.set_margin(position.y, 0, 0, position.x);
        layer_surface.set_exclusive_zone(0);
        layer_surface
            .set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        surface.commit();

        self.buffers = vec![create_buffer(shm, qh, 0)?, create_buffer(shm, qh, 1)?];
        self.surface = Some(surface);
        self.layer_surface = Some(layer_surface);
        self.running = true;
        Ok(())
    }

    fn draw_and_commit(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending {
            return;
        }
        let Some(index) = self.buffers.iter().position(|slot| !slot.busy) else {
            return;
        };

        let started_at = self.started_at.get_or_insert_with(Instant::now);
        let text = format_elapsed(started_at.elapsed());
        let mut pixels = vec![0_u32; WIDTH * HEIGHT];
        render_timer(&text, &mut pixels, self.colors);

        let slot = &mut self.buffers[index];
        let (destinations, remainder) = slot.memory.as_chunks_mut::<4>();
        debug_assert!(remainder.is_empty());
        for (destination, pixel) in destinations.iter_mut().zip(pixels) {
            destination.copy_from_slice(&pixel.to_ne_bytes());
        }
        let _ = slot.memory.flush_async();
        slot.busy = true;

        if let Some(surface) = self.surface.as_ref() {
            surface.attach(Some(&slot.proxy), 0, 0);
            surface.damage_buffer(0, 0, WIDTH as i32, HEIGHT as i32);
            surface.frame(qh, ());
            surface.commit();
            self.frame_pending = true;
        }
    }

    fn shutdown(&mut self) -> std::io::Result<()> {
        self.running = false;
        if let Some(relative_pointer) = self.relative_pointer.take() {
            relative_pointer.destroy();
        }
        if let Some(pointer) = self.pointer.take().filter(|pointer| pointer.version() >= 3) {
            pointer.release();
        }
        if let Some(seat) = self.seat.take().filter(|seat| seat.version() >= 5) {
            seat.release();
        }
        if let Some(manager) = self.relative_pointer_manager.take() {
            manager.destroy();
        }
        if let Some(layer_surface) = self.layer_surface.take() {
            layer_surface.destroy();
        }
        if let Some(surface) = self.surface.take() {
            surface.destroy();
        }
        for slot in self.buffers.drain(..) {
            slot.proxy.destroy();
        }
        self.placement.shutdown()
    }

    fn apply_position(&self, position: Position) {
        if let (Some(layer_surface), Some(surface)) =
            (self.layer_surface.as_ref(), self.surface.as_ref())
        {
            layer_surface.set_margin(position.y, 0, 0, position.x);
            surface.commit();
        }
    }

    fn ensure_relative_pointer(&mut self, qh: &QueueHandle<Self>) {
        if self.relative_pointer.is_some() {
            return;
        }
        if let (Some(manager), Some(pointer)) = (
            self.relative_pointer_manager.as_ref(),
            self.pointer.as_ref(),
        ) {
            self.relative_pointer = Some(manager.get_relative_pointer(pointer, qh, ()));
        }
    }

    fn remove_pointer(&mut self) {
        if let Some(relative_pointer) = self.relative_pointer.take() {
            relative_pointer.destroy();
        }
        if let Some(pointer) = self.pointer.take().filter(|pointer| pointer.version() >= 3) {
            pointer.release();
        }
        if let Err(error) = self.placement.release() {
            self.record_placement_error(error);
        }
    }

    fn record_placement_error(&mut self, error: std::io::Error) {
        if self.fatal_error.is_none() {
            self.fatal_error = Some(format!("cannot save placement: {error}"));
        }
        self.running = false;
    }

    fn check_error(&mut self) -> Result<(), Box<dyn Error>> {
        match self.fatal_error.take() {
            Some(error) => Err(std::io::Error::other(error).into()),
            None => Ok(()),
        }
    }
}

fn create_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<State>,
    index: usize,
) -> Result<BufferSlot, Box<dyn Error>> {
    let fd = memfd_create("wlapse-shm", MemfdFlags::CLOEXEC)?;
    ftruncate(&fd, BUFFER_BYTES as u64)?;
    let file = File::from(fd);
    // SAFETY: the file has exactly BUFFER_BYTES bytes and remains valid while the mapping is created.
    let memory = unsafe { MmapOptions::new().len(BUFFER_BYTES).map_mut(&file)? };
    let pool = shm.create_pool(file.as_fd(), BUFFER_BYTES as i32, qh, ());
    let proxy = pool.create_buffer(
        0,
        WIDTH as i32,
        HEIGHT as i32,
        (WIDTH * 4) as i32,
        wl_shm::Format::Argb8888,
        qh,
        index,
    );
    pool.destroy();
    Ok(BufferSlot {
        proxy,
        memory,
        busy: false,
    })
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };

        match interface.as_str() {
            "wl_compositor" if state.compositor.is_none() => {
                state.compositor = Some(registry.bind(name, version.min(4), qh, ()))
            }
            "wl_shm" if state.shm.is_none() => state.shm = Some(registry.bind(name, 1, qh, ())),
            "wl_seat" if state.seat.is_none() => {
                state.seat = Some(registry.bind(name, version.min(9), qh, ()))
            }
            "zwp_relative_pointer_manager_v1" if state.relative_pointer_manager.is_none() => {
                state.relative_pointer_manager = Some(registry.bind(name, 1, qh, ()));
                state.ensure_relative_pointer(qh);
            }
            "zwlr_layer_shell_v1" if state.layer_shell.is_none() => {
                state.layer_shell = Some(registry.bind(name, version.min(4), qh, ()))
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(State: ignore zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1);

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Pointer) {
            if state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
                state.ensure_relative_pointer(qh);
            }
        } else {
            state.remove_pointer();
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wl_pointer::Event::Button {
            button,
            state: WEnum::Value(button_state),
            ..
        } = event
        else {
            return;
        };
        if button != BTN_LEFT {
            return;
        }
        match button_state {
            wl_pointer::ButtonState::Pressed => state.placement.press(),
            wl_pointer::ButtonState::Released => {
                if let Err(error) = state.placement.release() {
                    state.record_placement_error(error);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<zwp_relative_pointer_v1::ZwpRelativePointerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &zwp_relative_pointer_v1::ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, .. } = event
            && let Some(position) = state.placement.motion(dx, dy)
        {
            state.apply_position(position);
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                layer_surface.ack_configure(serial);
                state.configured = true;
                state.draw_and_commit(qh);
            }
            zwlr_layer_surface_v1::Event::Closed => state.running = false,
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_callback::Event::Done { .. }) {
            state.frame_pending = false;
            state.draw_and_commit(qh);
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, usize> for State {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        index: &usize,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release) {
            if let Some(slot) = state.buffers.get_mut(*index) {
                slot.busy = false;
            }
            state.draw_and_commit(qh);
        }
    }
}
