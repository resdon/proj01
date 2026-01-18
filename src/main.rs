use pixels::{Pixels, SurfaceTexture}; 
use winit::application::ApplicationHandler; 
use winit::event_loop::{ActiveEventLoop, EventLoop}; 
use winit::window::{Window, WindowId};
use std::fs::File;
use std::os::unix::process;
use std::sync::Arc;
mod scan_dir;
use scan_dir::scan_dir;
use std::sync::mpsc::{channel, Receiver};
use std::collections::HashSet;
use std::path::PathBuf;
mod world;
use world::World;
use sysinfo::{Pid, System};

const WIDTH: u32 = 960;  
const HEIGHT: u32 = 540; 
const VISIBLE_COUNT: usize = 15;
const TRACK_Y: f32 = 70.0;
const TRACK_H: f32 = 375.0;
const ROW_H: u32 = 25; 
const LIST_X: f32 = 170.0;
const LIST_W: f32 = 700.0;
const FONT_DATA: &[u8] = include_bytes!("../font.ttf");

struct FileDisplay {
    prefix: String,
    name: String,
    size: String,
    mtime: String,
    atime: String,
    ctime: String,
}

struct MyApp {
    window: Option<Arc<Window>>,      
    pixels: Option<Pixels<'static>>,
    world: World,
    input_text: String,
    file_list: Vec<scan_dir::FileDisplay>,
    temp_list: Vec<scan_dir::FileDisplay>,
    receiver: Option<Receiver<scan_dir::ScanMsg>>,
    scroll_index: usize, 
    mouse_pos: (f32, f32),
    is_dragging_scrollbar: bool,
    selected_indices: HashSet<usize>, 
    context_menu: Option<(f32, f32)>,
    selection_start: Option<(f32, f32)>, 
    selection_rect: Option<(f32, f32, f32, f32)>,
    clipboard: Option<arboard::Clipboard>,
    properties_window: Option<(f32, f32)>, // New field for properties window position
    sort_prop: scan_dir::SortProperty,
    sort_order: scan_dir::SortOrder,
    show_hidden: bool,
    frame_count: u64,
    fps_timer: std::time::Instant,
    sys: sysinfo::System,
    cpu_usage: f32,
    ram_usage: u64,
    vram_usage: u64,
    pid: Pid,
}

fn trim_memory() {
    unsafe {
        libc::malloc_trim(0);
    }
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

    fn trigger_refresh(&mut self) {
        let (tx, rx) = channel::<scan_dir::ScanMsg>();

        self.world.clear_font_cache(); // Clear font bitmaps on refresh
        
        // Replace with empty vectors to drop the old allocated memory
        self.temp_list = Vec::new();
        self.file_list = Vec::new();

        trim_memory();
        
        self.scroll_index = 0;
        self.selected_indices.clear(); 
        self.receiver = Some(rx); // Dropping the old rx here triggers the return in scan_dir.rs
        
        scan_dir(
        self.input_text.clone(),
        tx, self.sort_prop, 
        self.sort_order, 
        self.show_hidden
        );

        if let Some(pixels) = &mut self.pixels {
            let _ = pixels.render();
        }
        unsafe {
            libc::malloc_trim(0);
        }

    }

    fn update_scroll_from_mouse(&mut self, my: f32) {
        if self.file_list.len() <= VISIBLE_COUNT { return; }
        let pct = ((my - TRACK_Y) / TRACK_H).clamp(0.0, 1.0);
        let max_scroll = self.file_list.len() - VISIBLE_COUNT;
        self.scroll_index = (pct * max_scroll as f32) as usize;
    }

    fn open_item(&mut self, index: usize) {
        if let Some(item) = self.file_list.get(index) {
            // No more split(';')! Access fields directly.
            let mut path = PathBuf::from(&self.input_text);
            path.push(&item.name); // Access the 'name' field

            if item.prefix == "[DIR]" { // Access the 'prefix' field
                self.input_text = path.to_string_lossy().into_owned();
                self.trigger_refresh();
            } else {
                // It's a file or a link, try to open it with the system default
                let _ = opener::open(path);
            }
        }
    }


    // Helper to get the current directory or the default sync path
    fn get_current_dir(&self) -> PathBuf {
        let path = PathBuf::from(&self.input_text);
        if path.exists() && path.is_dir() {
            path
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
    }

    // Parses the [TYPE];NAME;SIZE... format used in your scan_dir logic
    fn get_item_info(&self, index: usize) -> Option<(String, bool)> {
        // 1. Get the struct reference from the list
        let item = self.file_list.get(index)?;

        // 2. Access fields directly (No more split or collect!)
        let is_dir = item.prefix == "[DIR]";
        let name = item.name.clone();

        Some((name, is_dir))
    }

    // Helper for recursive directory copying
    fn copy_dir_recursive(&self, src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                self.copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()))?;
            } else {
                std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    fn cut_item(&mut self, indices: Vec<usize>) {
        let mut paths = Vec::new();
        for index in indices {
            if let Some((name, _)) = self.get_item_info(index) {
                let path = self.get_current_dir().join(name);
                paths.push(path.to_string_lossy().into_owned());
            }
        }
        if !paths.is_empty() {
            if let Some(ref mut cb) = self.clipboard {
                let _ = cb.set_text(format!("CUT:{}", paths.join("|")));
                // The clipboard object is NOT dropped here, so the OS keeps the data!
            }
        }
    }

    fn copy_item(&mut self, indices: Vec<usize>) {
        let mut paths = Vec::new();
        for index in indices {
            if let Some((name, _)) = self.get_item_info(index) {
                let path = self.get_current_dir().join(name);
                paths.push(path.to_string_lossy().into_owned());
            }
        }
        if !paths.is_empty() {
            if let Some(ref mut cb) = self.clipboard {
                let _ = cb.set_text(format!("COPY:{}", paths.join("|")));
            }
        }
    }

    fn paste_item(&mut self) {
        // Use the persistent clipboard
        let content = if let Some(ref mut cb) = self.clipboard {
            cb.get_text().ok()
        } else {
            None
        };

        if let Some(content) = content {
            let (is_cut, paths_raw) = if content.starts_with("CUT:") {
                (true, &content[4..])
            } else if content.starts_with("COPY:") {
                (false, &content[5..])
            } else {
                (false, content.as_str()) // Fallback for external paths
            };

            let dest_dir = self.get_current_dir();
            for path_str in paths_raw.split('|') {
                let src = std::path::PathBuf::from(path_str);
                if src.exists() {
                    let dest = dest_dir.join(src.file_name().unwrap_or_default());
                    if src == dest { continue; } // Avoid self-copy

                    let res = if src.is_dir() {
                        self.copy_dir_recursive(&src, &dest)
                    } else {
                        std::fs::copy(&src, &dest).map(|_| ())
                    };

                    if res.is_ok() && is_cut {
                        let _ = if src.is_dir() { std::fs::remove_dir_all(&src) } 
                                else { std::fs::remove_file(&src) };
                    }
                }
            }
            self.trigger_refresh();
        }
    }

    fn copy_path(&mut self, index: usize) {
        if let Some((name, _)) = self.get_item_info(index) {
            let path = self.get_current_dir().join(&name);
            let _ = arboard::Clipboard::new().ok().and_then(|mut cb| 
                cb.set_text(path.to_string_lossy().into_owned()).ok()
            );
        }
    }

    fn rename_item(&mut self, index: usize, new_name: &str) {
        if let Some((old_name, _)) = self.get_item_info(index) {
            let base = self.get_current_dir();
            let old_path = base.join(old_name);
            let new_path = base.join(new_name);
            
            if !new_path.exists() && std::fs::rename(old_path, new_path).is_ok() {
                self.trigger_refresh();
            }
        }
    }

    fn remove_item(&mut self, index: usize) {
        if let Some((name, is_dir)) = self.get_item_info(index) {
            let path = self.get_current_dir().join(name);
            let res = if is_dir {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            };
            
            if res.is_ok() {
                self.trigger_refresh();
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
            self.clipboard = arboard::Clipboard::new().ok();
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

                if button == winit::event::MouseButton::Right && state.is_pressed() {
                    // --- ADDED: AUTO-SELECT LOGIC ---
                    // 1. Check if the click is within the file list area
                    if mx >= LIST_X && mx <= LIST_X + LIST_W && my >= 70.0 && my <= (70.0 + TRACK_H) {
                        let row_idx = ((my - 70.0) / ROW_H as f32) as usize;
                        let actual_idx = self.scroll_index + row_idx;

                        // 2. If it's a valid file, select it
                        if actual_idx < self.file_list.len() {
                            // Only change selection if the item isn't already part of a multi-selection
                            if !self.selected_indices.contains(&actual_idx) {
                                self.selected_indices.clear();
                                self.selected_indices.insert(actual_idx);
                            }
                        }
                    }                
                                        
                    let menu_width = 120.0;
                    let menu_height = 150.0; // MATCH: Change this to match the new visual height (150)

                    // Auto-flip logic: ensures menu stays within bounds
                    let spawn_x = if mx + menu_width > WIDTH as f32 { mx - menu_width } else { mx };
                    let spawn_y = if my + menu_height > (HEIGHT - 70) as f32 { 
                        my - menu_height // Flip up if too close to the footer
                    } else { 
                        my 
                    };

                    self.context_menu = Some((spawn_x, spawn_y));
                }

                if state == winit::event::ElementState::Pressed {
                    match button {
                        winit::event::MouseButton::Left => {
                            // Properties window check
                            if let Some((cx, cy)) = self.properties_window {
                                let win_w = 240.0;
                                let win_h = 240.0;
                                if mx >= cx && mx <= cx + win_w && my >= cy && my <= cy + win_h {
                                    return; // Clicked inside properties window, do nothing
                                } else {
                                    self.properties_window = None; // Close properties if clicked outside
                                }
                            }
                            
                            // 1. Check Context Menu
                            if let Some((cx, cy)) = self.context_menu {

                                let is_on_open = mx >= cx && mx <= cx + 120.0 && my >= cy && my <= cy + 20.0;
                                let is_on_cut = mx >= cx && mx <= cx + 120.0 && my >= cy + 20.0 && my <= cy + 40.0;
                                let is_on_copy = mx >= cx && mx <= cx + 120.0 && my >= cy + 40.0 && my <= cy + 60.0;
                                let is_on_copy_path = mx >= cx && mx <= cx + 120.0 && my >= cy + 60.0 && my <= cy + 80.0;
                                let is_on_paste = mx >= cx && mx <= cx + 120.0 && my >= cy + 80.0 && my <= cy + 100.0;
                                let is_on_rename = mx >= cx && mx <= cx + 120.0 && my >= cy + 100.0 && my <= cy + 120.0;
                                let is_on_delete = mx >= cx && mx <= cx + 120.0 && my >= cy + 120.0 && my <= cy + 140.0;
                                let is_on_properties = mx >= cx && mx <= cx + 120.0 && my >= cy + 140.0 && my <= cy + 165.5;

                                if is_on_open {
                                    if let Some(&first_sel) = self.selected_indices.iter().next() {
                                        self.open_item(first_sel);
                                    }
                                    self.context_menu = None;
                                    return;
                                } else if is_on_cut {
                                    let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                    self.cut_item(selected);
                                    self.context_menu = None;
                                    return;
                                } else if is_on_copy {
                                    let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                    self.copy_item(selected);
                                    self.context_menu = None;
                                    return;
                                } else if is_on_copy_path {
                                    let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                    for &idx in &selected {
                                        self.copy_path(idx);
                                    }
                                    self.context_menu = None;
                                    return;
                                } else if is_on_paste {
                                    self.paste_item();
                                    self.context_menu = None;
                                    return;
                                } else if is_on_rename {
                                    let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                    for &idx in &selected {
                                        self.rename_item(idx, "renamed_item");
                                    }
                                    self.context_menu = None;
                                    return;
                                } else if is_on_delete {
                                    let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                    for &idx in &selected {
                                        self.remove_item(idx);
                                    }
                                    self.context_menu = None;
                                    return;
                                } else if is_on_properties {
                                    let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                    if let Some(&first_sel) = selected.first() {
                                        self.properties_window = Some((mx, my));
                                    }
                                }


                            }
                            self.context_menu = None;

                            // 2. Check UI Buttons (Don't start drag if clicking buttons)
                            let is_on_back = mx >= 10.0 && mx <= 85.0 && my >= 10.0 && my <= 40.0;
                            let is_on_refresh = mx >= 90.0 && mx <= 165.0 && my >= 10.0 && my <= 40.0;
                            let is_on_path = mx >= 170.0 && mx <= 870.0 && my >= 10.0 && my <= 40.0;
                            // Inside window_event MouseInput logic:
                            let is_on_open = mx >= 180.0 && mx <= 230.0 && my >= 470.0 && my <= 520.0;
                            let is_on_opw = mx >= 240.0 && mx <= 290.0 && my >= 470.0 && my <= 520.0;
                            let is_on_create = mx >= 300.0 && mx <= 350.0 && my >= 470.0 && my <= 520.0;
                            let is_on_blank = mx >= 360.0 && mx <= 410.0 && my >= 470.0 && my <= 520.0;
                            let is_on_cut = mx >= 420.0 && mx <= 470.0 && my >= 470.0 && my <= 520.0;
                            let is_on_copy = mx >= 480.0 && mx <= 530.0 && my >= 470.0 && my <= 520.0;
                            let is_on_copy_path = mx >= 540.0 && mx <= 590.0 && my >= 470.0 && my <= 520.0;
                            let is_on_paste = mx >= 600.0 && mx <= 650.0 && my >= 470.0 && my <= 520.0;
                            let is_on_rename = mx >= 660.0 && mx <= 710.0 && my >= 470.0 && my <= 520.0;
                            let is_on_delete = mx >= 720.0 && mx <= 770.0 && my >= 470.0 && my <= 520.0;
                            let is_on_properties = mx >= 780.0 && mx <= 830.0 && my >= 470.0 && my <= 520.0;

                            if is_on_back {
                                let mut path = PathBuf::from(&self.input_text);
                                if path.pop() { self.input_text = path.to_string_lossy().into_owned(); self.trigger_refresh(); }
                                return;
                            } else if is_on_refresh {
                                self.trigger_refresh();
                                return;
                            } else if is_on_path {
                                return;
                            } else if is_on_cut {
                                let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                self.cut_item(selected); // Now accepts Vec<usize>
                                return;
                            } else if is_on_copy {
                                let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                self.copy_item(selected); // Now accepts Vec<usize>
                                return;
                            } else if is_on_copy_path {
                                let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                for &idx in &selected {
                                    self.copy_path(idx);
                                }
                                return;
                            } else if is_on_paste {
                                self.paste_item();
                                return;
                            } else if is_on_rename {
                                let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                for &idx in &selected {
                                    self.rename_item(idx, "renamed_item"); // Placeholder new name
                                }
                                return;
                            } else if is_on_delete {
                                let selected: Vec<usize> = self.selected_indices.iter().cloned().collect();
                                for &idx in &selected {
                                    self.remove_item(idx);
                                }
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
                if let Some(ref rx) = self.receiver {
                        for msg in rx.try_iter() {
                            match msg {
                                scan_dir::ScanMsg::Clear => {
                                    self.temp_list = Vec::new();
                                    self.file_list = Vec::new();
                                }
                                scan_dir::ScanMsg::Entry(display_data) => {
                                    self.temp_list.push(display_data);
                                }
                                scan_dir::ScanMsg::Done => {
                                    self.file_list = std::mem::take(&mut self.temp_list);
                                }
                            }
                        }
                    }

                if let Some(pixels) = self.pixels.as_mut() {
                    let frame = pixels.frame_mut();
                    
                    // Use fill for better performance than chunks_exact_mut if clearing to one color
                    frame.fill(0); // Clear to black
                    // Or your specific color:
                    for pixel in frame.chunks_exact_mut(4) { pixel.copy_from_slice(&[20, 20, 20, 255]); }

                    let (mx, my) = self.mouse_pos;
                    let btn_hover = |x: f32, y: f32, w: f32, h: f32| mx >= x && mx <= x+w && my >= y && my <= y+h;

                    // Header Buttons
                    // Back
                    let b_col = if btn_hover(10.0, 10.0, 75.0, 30.0) { [0, 150, 150, 255] } else { [0, 100, 100, 255] };
                    Self::draw_rect(frame, 10, 10, 75, 30, b_col);
                    self.world.draw_text(frame, "Back", 15, 30, 18.0, [255, 255, 255]);
                    
                    // Refresh
                    let r_col = if btn_hover(90.0, 10.0, 75.0, 30.0) { [0, 150, 150, 255] } else { [0, 100, 100, 255] };
                    Self::draw_rect(frame, 90, 10, 75, 30, r_col);
                    self.world.draw_text(frame, "Refresh", 95, 30, 18.0, [255, 255, 255]);

                    // Path Bar
                    Self::draw_rect(frame, 170, 10, 700, 30, [200, 100, 0, 255]); 
                    self.world.draw_text(frame, &self.input_text, 175, 32, 18.0, [0, 0, 0]); 

                    // Sidebar
                    Self::draw_rect(frame, 10, 70, 150, 460, [40, 40, 40, 255]); 

                    // Sort Options
                    Self::draw_rect(frame, 170, 45, 55, 20, [150, 0, 255, 255]);
                    Self::draw_rect(frame, 230, 45, 155, 20, [150, 0, 255, 255]);
                    Self::draw_rect(frame, 390, 45, 195, 20, [150, 0, 255, 255]);
                    Self::draw_rect(frame, 590, 45, 95, 20, [150, 0, 255, 255]);
                    Self::draw_rect(frame, 690, 45, 95, 20, [150, 0, 255, 255]);
                    Self::draw_rect(frame, 790, 45, 80, 20, [150, 0, 255, 255]);

                    // File List
                    for i in 0..VISIBLE_COUNT {
                        let actual_idx = self.scroll_index + i;
                        if actual_idx >= self.file_list.len() { break; }
                        let y_pos = 70 + (i as u32 * ROW_H);
                        let is_selected = self.selected_indices.contains(&actual_idx);
                        let is_hovered = mx >= LIST_X && mx <= LIST_X + LIST_W && my >= y_pos as f32 && my < (y_pos + 22) as f32;
                        
                        let col = if is_selected { [180, 0, 0, 255] } else if is_hovered { [60, 60, 60, 255] } else { [35, 35, 35, 255] };
                        Self::draw_rect(frame, LIST_X as u32, y_pos, LIST_W as u32, 22, col);
                        
                        let item = &self.file_list[actual_idx];
                        self.world.draw_text(frame, &item.prefix, 180, (y_pos + 16) as usize, 12.0, [255, 215, 0]);
                        self.world.draw_text(frame, &item.name, 240, (y_pos + 16) as usize, 12.0, [255, 255, 255]);
                        self.world.draw_text(frame, &item.size, 400, (y_pos + 16) as usize, 12.0, [0, 255, 255]);
                        self.world.draw_text(frame, &item.mtime, 600, (y_pos + 16) as usize, 12.0, [200, 200, 200]);
                        self.world.draw_text(frame, &item.atime, 700, (y_pos + 16) as usize, 12.0, [200, 200, 200]);
                        self.world.draw_text(frame, &item.ctime, 800, (y_pos + 16) as usize, 12.0, [200, 200, 200]);
                    }

                    // Selection Rectangle (Drawn on top)
                    if let Some((rx, ry, rw, rh)) = self.selection_rect {
                        Self::draw_rect(frame, rx as u32, ry as u32, rw as u32, rh as u32, [0, 120, 215, 80]);
                    }

                    // Context Menu (Drawn on top)
                    if let Some((cx, cy)) = self.context_menu {
                        let menu_w = 120;
                        let menu_h = 170; // CHANGE: Reduced from 480 to 150
                        let bg_color = [0, 0, 0, 255]; // CHANGE: Darker navy/slate color

                        // Draw main background
                        Self::draw_rect(frame, cx as u32, cy as u32, menu_w, menu_h, bg_color);

                        // Draw items with tighter spacing (e.g., 30px apart)
                        let items = ["Open", "Cut", "Copy", "Copy Path", "Paste", "Rename", "Delete", "Properties"];
                        for (i, text) in items.iter().enumerate() {
                            let item_y = cy as u32 + (i as u32 * 20);
                            // Optional: Draw a hover effect or separator here
                            self.world.draw_text(frame, text, (cx as usize) + 10, (item_y + 20) as usize, 13.0, [255, 255, 255]);
                        }
                    }

                    // Properties Window (Drawn on top)
                    if let Some((_cx, _cy)) = self.properties_window {
                        let win_w: u32 = 240;
                        let win_h: u32 = 240;
                        Self::draw_rect(frame, 360 as u32, 150 as u32, win_w, win_h, [50, 50, 50, 255]);
                        self.world.draw_text(frame, "Properties", 490, 290 + 20, 18.0, [255, 255, 255]);
                        // Additional properties details can be drawn here
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

                    // Operations footer
                    Self::draw_rect(frame, 170, 460, 700, 70, [200, 100, 0, 255]);
                    // Operations buttons
                    Self::draw_rect(frame, 180, 470, 50, 50, [255, 0, 0, 255]);
                    self.world.draw_text(frame, "Opn", 185, 500, 20.0, [255, 255, 255]);
                    Self::draw_rect(frame, 240, 470, 50, 50, [0, 255, 0, 255]);
                    self.world.draw_text(frame, "Opw", 245, 500, 20.0, [255, 255, 255]);
                    Self::draw_rect(frame, 300, 470, 50, 50, [0, 0, 255, 255]);
                    self.world.draw_text(frame, "Cr", 310, 500, 20.0, [255, 255, 255]);
                    Self::draw_rect(frame, 360, 470, 50, 50, [255, 255, 0, 255]);
                    self.world.draw_text(frame, "", 370, 500, 20.0, [255, 255, 255]);

                    // Cut, Copy, CopyPath, Paste, Rename
                    // Cut 420
                    Self::draw_rect(frame, 420, 470, 50, 50, [0, 255, 255, 255]);
                    self.world.draw_text(frame, "Cut", 430, 500, 20.0, [255, 255, 255]);
                    // Copy 480
                    Self::draw_rect(frame, 480, 470, 50, 50, [255, 0, 255, 255]);
                    self.world.draw_text(frame, "CP", 490, 500, 20.0, [255, 255, 255]);
                    // Copy Path 540
                    Self::draw_rect(frame, 540, 470, 50, 50, [192, 192, 192, 255]);
                    self.world.draw_text(frame, "CPp", 550, 500, 20.0, [255, 255, 255]);
                    // Paste 600
                    Self::draw_rect(frame, 600, 470, 50, 50, [128, 0, 128, 255]);
                    self.world.draw_text(frame, "Pst", 610, 500, 20.0, [255, 255, 255]);
                    // Rename 660
                    Self::draw_rect(frame, 660, 470, 50, 50, [0, 128, 128, 255]);
                    self.world.draw_text(frame, "Ren", 670, 500, 20.0, [255, 255, 255]);
                    // Delete 720
                    Self::draw_rect(frame, 720, 470, 50, 50, [128, 128, 0, 255]);
                    self.world.draw_text(frame, "Del", 730, 500, 20.0, [255, 255, 255]);
                    // Properties
                    Self::draw_rect(frame, 780, 470, 50, 50, [0, 0, 0, 255]);
                    self.world.draw_text(frame, "Prp", 790, 500, 20.0, [255, 255, 255]);

                    // Performance Metrics
                    self.frame_count += 1;
                    if self.fps_timer.elapsed().as_secs() >= 1 {
                        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[self.pid]), true);

                        if let Some(process) = self.sys.process(self.pid) {
                            self.cpu_usage = process.cpu_usage();
                            self.ram_usage = process.memory() / 1024 / 1024;
                            self.vram_usage = process.virtual_memory() / 1024 / 1024;
                        }

                        println!("FPS: {}; CPU: {:.1}%; RAM: {}MB; VRAM: {}MB", 
                        self.frame_count, 
                        self.cpu_usage,
                        self.ram_usage,
                        self.vram_usage);
                        self.frame_count = 0;
                        self.fps_timer = std::time::Instant::now();
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
    let event_loop = EventLoop::new().unwrap();
    let world = World::new(WIDTH as usize, HEIGHT as usize, FONT_DATA);
    let mut sys = System::new_all();
    sys.refresh_all();
    let pid = sysinfo::get_current_pid().expect("Failed to get PID");
    let mut app = MyApp { 
        window: None, 
        pixels: None, 
        world, 
        input_text: std::env::current_dir().unwrap_or_default().to_string_lossy().into_owned(), 
        file_list: Vec::new(), 
        temp_list: Vec::new(), 
        receiver: None, 
        scroll_index: 0, 
        mouse_pos: (0.0, 0.0), 
        is_dragging_scrollbar: false, 
        selected_indices: HashSet::new(), 
        context_menu: None, 
        selection_start: None, 
        selection_rect: None, 
        clipboard: None, 
        properties_window: None,
        sort_prop: scan_dir::SortProperty::Name,
        sort_order: scan_dir::SortOrder::Asc,
        show_hidden: false, 
        fps_timer: std::time::Instant::now(), frame_count: 0,
        sys, cpu_usage: 0.0, ram_usage: 0, vram_usage: 0, pid,
    }; 
    event_loop.run_app(&mut app).unwrap(); 
}