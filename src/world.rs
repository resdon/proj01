use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;

pub struct CachedGlyph {
    pub bitmap: Vec<u8>,
    pub metrics: Metrics,
}

pub struct FontManager {
    font: Font,
    cache: HashMap<(char, u32), CachedGlyph>,
}

impl FontManager {
    pub fn new(font_data: &[u8]) -> Self {
        let font = Font::from_bytes(font_data, FontSettings::default()).expect("Invalid font data");
        Self {
            font,
            cache: HashMap::new(),
        }
    }

    pub fn get_glyph(&mut self, character: char, size: f32) -> &CachedGlyph {
        let key = (character, size as u32);

        // If we have more than 2000 characters cached, clear it automatically
        if self.cache.len() > 5000 {
            self.cache.clear();
            self.cache.shrink_to_fit();
        }

        self.cache.entry(key).or_insert_with(|| {
            let (metrics, bitmap) = self.font.rasterize(character, size);
            CachedGlyph {
                bitmap,
                metrics,
            }
        })
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.cache.shrink_to_fit();
    }

}

pub struct World {
    pub font_manager: FontManager,
    pub width: usize,
    pub height: usize,
}

impl World {

    pub fn new(width: usize, height: usize, font_data: &[u8]) -> Self {
        Self {
            font_manager: FontManager::new(font_data),
            width,
            height,
        }
    }

    pub fn draw_text(
        &mut self,
        frame: &mut [u8],
        text: &str,
        start_x: usize,
        baseline_y: usize,
        size: f32,
        color: [u8; 3],
    ) {
        let mut cursor_x = start_x;
        // Capture dimensions here so we don't need &self inside the loop for blitting
        let screen_width = self.width;
        let screen_height = self.height;

        for c in text.chars() {
            // 1. Mutably borrow self to get/cache the glyph
            let glyph = self.font_manager.get_glyph(c, size);
            let advance = glyph.metrics.advance_width as usize;

            // 2. Pass the glyph and explicit dimensions to the static helper
            // We do NOT use 'self.blit_glyph' here to avoid double-borrowing
            Self::blit_glyph(frame, screen_width, screen_height, glyph, cursor_x, baseline_y, color);

            cursor_x += advance;

            if cursor_x >= screen_width { break; }
        }
    }

    // CHANGED: Removed '&self'. Added 'width' and 'height' args.
    fn blit_glyph(
        frame: &mut [u8], 
        width: usize, 
        height: usize, 
        glyph: &CachedGlyph, 
        x: usize, 
        y: usize, 
        color: [u8; 3]
    ) {
        let metrics = &glyph.metrics;
        let g_width = metrics.width;
        let g_height = metrics.height;

        for row in 0..g_height {
            for col in 0..g_width {
                let opacity = glyph.bitmap[row * g_width + col];
                if opacity == 0 { continue; }

                let target_x = (x as i32 + metrics.xmin + col as i32) as isize;
                let target_y = (y as i32 - metrics.ymin - g_height as i32 + row as i32) as isize;

                if target_x >= 0 && target_x < width as isize && target_y >= 0 && target_y < height as isize {
                    // FIX: Removed unnecessary parentheses
                    let pixel_index = (target_y as usize * width + target_x as usize) * 4;
                    
                    frame[pixel_index] = color[0];
                    frame[pixel_index + 1] = color[1];
                    frame[pixel_index + 2] = color[2];
                    frame[pixel_index + 3] = opacity;
                }
            }
        }
    }

    pub fn clear_font_cache(&mut self) {
        self.font_manager.clear_cache();
        self.font_manager.cache.shrink_to_fit();
    }

}