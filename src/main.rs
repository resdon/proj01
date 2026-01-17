use pixels::{Pixels, SurfaceTexture}; 
use winit::application::ApplicationHandler; 
use winit::event_loop::{ActiveEventLoop, EventLoop}; 
use winit::window::{Window, WindowId};
use std::sync::Arc;
mod scan_dir;
use scan_dir::scan_dir;
use std::sync::mpsc::{channel, Receiver};
use std::collections::HashSet;
use std::path::PathBuf;

const WIDTH: u32 = 960;  
const HEIGHT: u32 = 540; 
const VISIBLE_COUNT: usize = 15;
const TRACK_Y: f32 = 70.0;
const TRACK_H: f32 = 375.0;
const ROW_H: u32 = 25; 
const LIST_X: f32 = 170.0;
const LIST_W: f32 = 700.0;

struct MyApp {
    window: Option<Arc<Window>>,      
    pixels: Option<Pixels<'static>>,
    font: fontdue::Font,
    input_text: String,
    file_list: Vec<String>,
    temp_list: Vec<String>,
    receiver: Option<Receiver<String>>,
    scroll_index: usize, 
    mouse_pos: (f32, f32),
    is_dragging_scrollbar: bool,
    selected_indices: HashSet<usize>, 
    context_menu: Option<(f32, f32)>,
    selection_start: Option<(f32, f32)>, 
    selection_rect: Option<(f32, f32, f32, f32)>, 
}

impl MyApp {
    fn draw_rect(frame: &mut [u8], x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
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

    fn draw_text(frame: &mut [u8], font: &fontdue::Font, text: &str, start_x: u32, start_y: u32, size: f32, color: [u8; 3]) {
        let mut x_cursor = start_x;
        for char in text.chars() {
            let (metrics, bitmap) = font.rasterize(char, size);
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let opacity = bitmap[row * metrics.width + col];
                    if opacity > 0 {
                        let target_x = x_cursor as i32 + metrics.xmin + col as i32;
                        let target_y = start_y as i32 - metrics.ymin - metrics.height as i32 + row as i32;
                        if target_x >= 0 && target_x < WIDTH as i32 && target_y >= 0 && target_y < HEIGHT as i32 {
                            let index = ((target_y as u32 * WIDTH + target_x as u32) * 4) as usize;
                            frame[index] = color[0];
                            frame[index + 1] = color[1];
                            frame[index + 2] = color[2];
                            frame[index + 3] = opacity;
                        }
                    }
                }
            }
            x_cursor += metrics.advance_width as u32;
        }
    }

    fn trigger_refresh(&mut self) {
        let (tx, rx) = channel();
        self.temp_list.clear();
        self.file_list.clear();
        self.scroll_index = 0;
        self.selected_indices.clear(); 
        self.receiver = Some(rx);
        scan_dir(self.input_text.clone(), tx);
    }

    fn update_scroll_from_mouse(&mut self, my: f32) {
        if self.file_list.len() <= VISIBLE_COUNT { return; }
        let pct = ((my - TRACK_Y) / TRACK_H).clamp(0.0, 1.0);
        let max_scroll = self.file_list.len() - VISIBLE_COUNT;
        self.scroll_index = (pct * max_scroll as f32) as usize;
    }

    fn open_item(&mut self, index: usize) {
        if let Some(line) = self.file_list.get(index) {
            let parts: Vec<&str> = line.split(';').collect();
            if parts.len() >= 2 {
                let mut path = PathBuf::from(&self.input_text);
                path.push(parts[1]);
                if parts[0] == "[DIR]" {
                    self.input_text = path.to_string_lossy().into_owned();
                    self.trigger_refresh();
                } else {
                    let _ = opener::open(path);
                }
            }
        }
    }
}

impl ApplicationHandler for MyApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window_attrs = Window::default_attributes()
                .with_title("File Explorer - Global Drag Selection")
                .with_inner_size(winit::dpi::LogicalSize::new(WIDTH, HEIGHT));
            let window = Arc::new(event_loop.create_window(window_attrs).unwrap());
            let surface = SurfaceTexture::new(WIDTH, HEIGHT, window.clone());
            let pixels = Pixels::new(WIDTH, HEIGHT, surface).expect("Pixels error");
            self.window = Some(window);
            self.pixels = Some(pixels);
            self.trigger_refresh();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: winit::event::WindowEvent) {
        match event {
            winit::event::WindowEvent::CloseRequested => event_loop.exit(),
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                let (mx, my) = (position.x as f32, position.y as f32);
                self.mouse_pos = (mx, my);
                if self.is_dragging_scrollbar {
                    self.update_scroll_from_mouse(my);
                } else if let Some((sx, sy)) = self.selection_start {
                    let rx = sx.min(mx); let ry = sy.min(my);
                    let rw = (sx - mx).abs(); let rh = (sy - my).abs();
                    self.selection_rect = Some((rx, ry, rw, rh));
                    
                    self.selected_indices.clear();
                    for i in 0..VISIBLE_COUNT {
                        let actual_idx = self.scroll_index + i;
                        if actual_idx >= self.file_list.len() { break; }
                        let row_y = 70.0 + (i as f32 * ROW_H as f32);
                        // Intersection check: selection box vs list row
                        if ry < row_y + ROW_H as f32 && ry + rh > row_y && rx < LIST_X + LIST_W && rx + rw > LIST_X {
                            self.selected_indices.insert(actual_idx);
                        }
                    }
                }
            }
            winit::event::WindowEvent::MouseInput { state, button, .. } => {
                let (mx, my) = self.mouse_pos;
                if state == winit::event::ElementState::Pressed {
                    match button {
                        winit::event::MouseButton::Left => {
                            // 1. Check Context Menu
                            if let Some((cx, cy)) = self.context_menu {
                                if mx >= cx && mx <= cx + 120.0 && my >= cy && my <= cy + 60.0 {
                                    if let Some(&first_sel) = self.selected_indices.iter().next() {
                                        self.open_item(first_sel);
                                    }
                                    self.context_menu = None;
                                    return;
                                }
                            }
                            self.context_menu = None;

                            // 2. Check UI Buttons (Don't start drag if clicking buttons)
                            let is_on_back = mx >= 10.0 && mx <= 85.0 && my >= 10.0 && my <= 40.0;
                            let is_on_refresh = mx >= 90.0 && mx <= 165.0 && my >= 10.0 && my <= 40.0;
                            let is_on_path = mx >= 170.0 && mx <= 870.0 && my >= 10.0 && my <= 40.0;

                            if is_on_back {
                                let mut path = PathBuf::from(&self.input_text);
                                if path.pop() { self.input_text = path.to_string_lossy().into_owned(); self.trigger_refresh(); }
                                return;
                            } else if is_on_refresh {
                                self.trigger_refresh();
                                return;
                            } else if is_on_path {
                                return;
                            }

                            // 3. Check Scrollbar
                            if mx >= 880.0 && mx <= 895.0 && my >= TRACK_Y && my <= (TRACK_Y + TRACK_H) {
                                self.is_dragging_scrollbar = true;
                                self.update_scroll_from_mouse(my);
                            } else {
                                // 4. START SELECTION ANYWHERE ELSE
                                // If we click a specific item, allow double-click/single-select
                                let row_idx = ((my - 70.0) / ROW_H as f32) as usize;
                                let actual_idx = self.scroll_index + row_idx;
                                
                                if mx >= LIST_X && mx <= LIST_X + LIST_W && my >= 70.0 && actual_idx < self.file_list.len() {
                                    if self.selected_indices.contains(&actual_idx) && self.selected_indices.len() == 1 {
                                        self.open_item(actual_idx);
                                    } else {
                                        self.selection_start = Some((mx, my));
                                        self.selected_indices.clear();
                                        self.selected_indices.insert(actual_idx);
                                    }
                                } else {
                                    // Clicked empty space
                                    self.selection_start = Some((mx, my));
                                    self.selected_indices.clear();
                                }
                            }
                        }
                        winit::event::MouseButton::Right => {
                            if mx >= LIST_X && mx <= LIST_X + LIST_W && my >= 70.0 {
                                let row_idx = ((my - 70.0) / ROW_H as f32) as usize;
                                let actual_idx = self.scroll_index + row_idx;
                                if actual_idx < self.file_list.len() {
                                    if !self.selected_indices.contains(&actual_idx) {
                                        self.selected_indices.clear();
                                        self.selected_indices.insert(actual_idx);
                                    }
                                    self.context_menu = Some((mx, my));
                                }
                            }
                        }
                        _ => {}
                    }
                } else {
                    self.is_dragging_scrollbar = false;
                    self.selection_start = None;
                    self.selection_rect = None;
                }
            }
            winit::event::WindowEvent::MouseWheel { delta, .. } => {
                if let winit::event::MouseScrollDelta::LineDelta(_, y) = delta {
                    if y > 0.0 { self.scroll_index = self.scroll_index.saturating_sub(1); }
                    else if !self.file_list.is_empty() && self.scroll_index + VISIBLE_COUNT < self.file_list.len() {
                        self.scroll_index += 1;
                    }
                }
            }
            winit::event::WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed() {
                    let mut text_changed = false;
                    match event.logical_key {
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace) => { self.input_text.pop(); text_changed = true; }
                        _ => if let Some(txt) = event.text {
                            for c in txt.chars() { if !c.is_control() { self.input_text.push(c); text_changed = true; } }
                        }
                    }
                    if text_changed { self.trigger_refresh(); }
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                if let Some(pixels) = self.pixels.as_mut() {
                    if let Some(ref rx) = self.receiver {
                        for msg in rx.try_iter() {
                            match msg.as_str() {
                                "__CLEAR__" => self.temp_list.clear(),
                                "__DONE__" => self.file_list = self.temp_list.clone(),
                                _ => self.temp_list.push(msg),
                            }
                        }
                    }
                    let frame = pixels.frame_mut();
                    for pixel in frame.chunks_exact_mut(4) { pixel.copy_from_slice(&[20, 20, 20, 255]); }

                    let (mx, my) = self.mouse_pos;
                    let btn_hover = |x: f32, y: f32, w: f32, h: f32| mx >= x && mx <= x+w && my >= y && my <= y+h;

                    // Header Buttons
                    let b_col = if btn_hover(10.0, 10.0, 75.0, 30.0) { [0, 150, 150, 255] } else { [0, 100, 100, 255] };
                    Self::draw_rect(frame, 10, 10, 75, 30, b_col);
                    Self::draw_text(frame, &self.font, "Back", 15, 30, 18.0, [255, 255, 255]);

                    let r_col = if btn_hover(90.0, 10.0, 75.0, 30.0) { [0, 150, 150, 255] } else { [0, 100, 100, 255] };
                    Self::draw_rect(frame, 90, 10, 75, 30, r_col);
                    Self::draw_text(frame, &self.font, "Refresh", 95, 30, 18.0, [255, 255, 255]);

                    // Path Bar & Sidebar
                    Self::draw_rect(frame, 170, 10, 700, 30, [255, 255, 255, 255]); 
                    Self::draw_text(frame, &self.font, &self.input_text, 175, 32, 18.0, [0, 0, 0]); 
                    Self::draw_rect(frame, 10, 70, 150, 460, [40, 40, 40, 255]); 

                    // File List
                    for i in 0..VISIBLE_COUNT {
                        let actual_idx = self.scroll_index + i;
                        if actual_idx >= self.file_list.len() { break; }
                        let y_pos = 70 + (i as u32 * ROW_H);
                        let is_selected = self.selected_indices.contains(&actual_idx);
                        let is_hovered = mx >= LIST_X && mx <= LIST_X + LIST_W && my >= y_pos as f32 && my < (y_pos + 22) as f32;
                        
                        let col = if is_selected { [180, 0, 0, 255] } else if is_hovered { [60, 60, 60, 255] } else { [35, 35, 35, 255] };
                        Self::draw_rect(frame, LIST_X as u32, y_pos, LIST_W as u32, 22, col);
                        
                        let parts: Vec<&str> = self.file_list[actual_idx].split(';').collect();
                        if parts.len() >= 2 {
                            Self::draw_text(frame, &self.font, parts[0], 180, y_pos + 16, 12.0, [255, 215, 0]);
                            Self::draw_text(frame, &self.font, parts[1], 240, y_pos + 16, 12.0, [255, 255, 255]);
                        }
                    }

                    // Selection Rectangle (Drawn on top)
                    if let Some((rx, ry, rw, rh)) = self.selection_rect {
                        Self::draw_rect(frame, rx as u32, ry as u32, rw as u32, rh as u32, [0, 120, 215, 80]);
                    }

                    // Context Menu (Drawn on top)
                    if let Some((cx, cy)) = self.context_menu {
                        Self::draw_rect(frame, cx as u32, cy as u32, 120, 60, [50, 50, 50, 255]);
                        Self::draw_text(frame, &self.font, "Open", cx as u32 + 10, cy as u32 + 25, 14.0, [255, 255, 255]);
                    }

                    // Scrollbar
                    if self.file_list.len() > VISIBLE_COUNT {
                        let total = self.file_list.len() as f32;
                        let thumb_h = (VISIBLE_COUNT as f32 / total * TRACK_H).max(30.0);
                        let scroll_pct = self.scroll_index as f32 / (total - VISIBLE_COUNT as f32);
                        let thumb_y = TRACK_Y + (scroll_pct * (TRACK_H - thumb_h));
                        Self::draw_rect(frame, 880, TRACK_Y as u32, 12, TRACK_H as u32, [45, 45, 45, 255]); 
                        let thumb_col = if self.is_dragging_scrollbar { [200, 200, 200, 255] } else { [120, 120, 120, 255] };
                        Self::draw_rect(frame, 880, thumb_y as u32, 12, thumb_h as u32, thumb_col);
                    }

                    pixels.render().unwrap();
                }
            }
            _ => (),
        }
        if let Some(window) = &self.window { window.request_redraw(); }
    }
}

fn main() {
    let font_data = include_bytes!("../font.ttf") as &[u8];
    let font = fontdue::Font::from_bytes(font_data, fontdue::FontSettings::default()).expect("Font error");
    let event_loop = EventLoop::new().unwrap();
    let mut app = MyApp { 
        window: None, pixels: None, font, 
        input_text: std::env::current_dir().unwrap_or_default().to_string_lossy().into_owned(), 
        file_list: Vec::new(), temp_list: Vec::new(), receiver: None, scroll_index: 0, 
        mouse_pos: (0.0, 0.0), is_dragging_scrollbar: false, selected_indices: HashSet::new(), 
        context_menu: None, selection_start: None, selection_rect: None,
    }; 
    event_loop.run_app(&mut app).unwrap(); 
}