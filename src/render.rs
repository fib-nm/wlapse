use crate::config::Colors;

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 64;

const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
const SCALE: usize = 4;
const SPACING: usize = 4;

pub fn render_timer(text: &str, pixels: &mut [u32], colors: Colors) {
    if pixels.len() != WIDTH * HEIGHT {
        return;
    }
    pixels.fill(colors.background);

    let text_width = text.chars().count() * GLYPH_WIDTH * SCALE
        + text.chars().count().saturating_sub(1) * SPACING;
    let origin_x = (WIDTH.saturating_sub(text_width)) / 2;
    let origin_y = (HEIGHT - GLYPH_HEIGHT * SCALE) / 2;

    for (index, character) in text.chars().enumerate() {
        let glyph = glyph(character);
        let glyph_x = origin_x + index * (GLYPH_WIDTH * SCALE + SPACING);
        for (row, bits) in glyph.iter().copied().enumerate() {
            for column in 0..GLYPH_WIDTH {
                if bits & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                    continue;
                }
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        let x = glyph_x + column * SCALE + dx;
                        let y = origin_y + row * SCALE + dy;
                        pixels[y * WIDTH + x] = colors.text;
                    }
                }
            }
        }
    }
}

fn glyph(character: char) -> [u8; GLYPH_HEIGHT] {
    match character {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ':' => [0, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0],
        '.' => [0, 0, 0, 0, 0, 0b00100, 0b00100],
        _ => [0; GLYPH_HEIGHT],
    }
}

#[cfg(test)]
mod tests {
    use super::{HEIGHT, WIDTH, render_timer};
    use crate::config::Colors;

    #[test]
    fn renders_timer_into_fixed_argb_buffer() {
        let mut pixels = vec![0_u32; WIDTH * HEIGHT];
        let colors = Colors::default();
        render_timer("00:00:00.0", &mut pixels, colors);

        assert!(pixels.contains(&colors.background));
        assert!(pixels.contains(&colors.text));
        assert!(
            pixels
                .iter()
                .all(|pixel| *pixel == colors.background || *pixel == colors.text)
        );
    }

    #[test]
    fn renders_with_configured_colors() {
        let mut pixels = vec![0_u32; WIDTH * HEIGHT];
        let colors = Colors {
            background: 0xFF11_2233,
            text: 0xFFAA_BBCC,
        };

        render_timer("00:00:00.0", &mut pixels, colors);

        assert!(pixels.contains(&colors.background));
        assert!(pixels.contains(&colors.text));
        assert!(
            pixels
                .iter()
                .all(|pixel| matches!(*pixel, 0xFF11_2233 | 0xFFAA_BBCC))
        );
    }
}
