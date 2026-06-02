use crate::WIDTH;
use crate::HEIGHT;

pub fn draw_rect(frame: &mut [u8], x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    for row in 0..h {
        for col in 0..w {
            let target_x = x + col;
            let target_y = y + row;
            if target_x < WIDTH && target_y < HEIGHT {
                let index = ((target_y * WIDTH + target_x) * 4) as usize;
                if color[3] == 255 {
                    frame[index..index + 4].copy_from_slice(&color);
                } else {
                    let alpha = color[3] as f32 / 255.0;
                    for i in 0..3 {
                        frame[index + i] = ((color[i] as f32 * alpha) + (frame[index + i] as f32 * (1.0 - alpha))) as u8;
                    }
                }
            }
        }
    }
}
