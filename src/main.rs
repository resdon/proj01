
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

mod world;

use world::World;
use sysinfo::{Pid, System};

mod open_with;
mod file_ops;
mod ui_components;

const WIDTH: u32 = 960;  
const HEIGHT: u32 = 540; 
const VISIBLE_COUNT: usize = 15;
const TRACK_Y: f32 = 70.0;
const TRACK_H: f32 = 375.0;
const ROW_H: u32 = 25; 
const LIST_X: f32 = 170.0;
const LIST_W: f32 = 700.0;
const FONT_DATA: &[u8] = include_bytes!("../font.ttf");

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
    pending_paths: Vec<PathBuf>,
    properties_window: Option<(usize, f32, f32)>, // New field for properties window position
    properties_pos: Option<(f32, f32)>,
    is_dragging_properties: bool,
    properties_drag_offset: (f32, f32),
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
    installed_apps: Vec<open_with::AppEntry>,
    show_open_with_menu: bool,
    open_with_menu_pos: Option<(f32, f32)>,
    pub open_with_window: Option<(f32, f32)>, // Position of the Open With window
    open_with_scroll: usize,
    pub open_with_search: String,
    is_dragging_open_with: bool,
    open_with_drag_offset: (f32, f32),
    search_query: String,  // To filter the app list
    pub open_with_selection: Option<usize>, // Index of the selected app in installed_apps
}

fn trim_memory() {
    unsafe {
        libc::malloc_trim(0);
    }
}

impl MyApp {
    fn execute_file(&self, path: &PathBuf, app_cmd: Option<&str>) {
        println!("Executing: {:?} with command: {:?}", path, app_cmd); 
        
        if let Some(cmd) = app_cmd {
            // Assign the result of .status() to the 'status' variable
            let status = std::process::Command::new("./src/open_with.sh")
                .arg(cmd)
                .arg(path.to_str().unwrap_or(""))
                .status(); 
                
            // Now 'status' is defined and safe to print
            println!("Bash result: {:?}", status);
        } else {
            let _ = std::process::Command::new("xdg-open")
                .arg(path)
                .spawn();
        }
    }

    fn close_all_menus(&mut self) {
        self.context_menu = None;
        self.show_open_with_menu = false;
    }
    fn update_open_with_window(&mut self, mx: f32, my: f32) {
        if let Some((_, _)) = self.open_with_window {
            let new_x = mx - self.open_with_drag_offset.0;
            let new_y = my - self.open_with_drag_offset.1;
            self.open_with_window = Some((new_x, new_y));
        }
    }
    fn handle_cursor_moved(&mut self, mx: f32, my: f32) {
        self.mouse_pos = (mx, my);

        if self.is_dragging_scrollbar {
            self.update_scroll_from_mouse(my);
        } else if self.selection_start.is_some() {
            self.update_selection_rect(mx, my);
        } else if self.is_dragging_properties {
            self.update_properties_window(mx, my);
        } else if self.is_dragging_open_with {
            self.update_open_with_window(mx, my);
        }
    }

    fn update_selection_rect(&mut self, mx: f32, my: f32) {
        if let Some((sx, sy)) = self.selection_start {
            let rx = sx.min(mx);
            let ry = sy.min(my);
            let rw = (sx - mx).abs();
            let rh = (sy - my).abs();
            self.selection_rect = Some((rx, ry, rw, rh));
            
            // Re-apply your intersection logic here
            self.selected_indices.clear();
            for i in 0..VISIBLE_COUNT {
                let actual_idx = self.scroll_index + i;
                if actual_idx >= self.file_list.len() { break; }
                let row_y = 70.0 + (i as f32 * ROW_H as f32);
                if ry < row_y + ROW_H as f32 && ry + rh > row_y && rx < LIST_X + LIST_W && rx + rw > LIST_X {
                    self.selected_indices.insert(actual_idx);
                }
            }
        }
    }

    fn update_properties_window(&mut self, mx: f32, my: f32) {
        if let Some((idx, _, _)) = self.properties_window {
            let new_x = mx - self.properties_drag_offset.0;
            let new_y = my - self.properties_drag_offset.1;
            self.properties_window = Some((idx, new_x, new_y));
        }
    }    

    fn is_menu_open(&self) -> bool {
        self.context_menu.is_some() || self.show_open_with_menu
    }
    fn is_inside_menu(&self, mx: f32, my: f32) -> bool {
        // Check main context menu
        if let Some((cx, cy)) = self.context_menu {
            if mx >= cx && mx <= cx + 120.0 && my >= cy && my <= cy + 185.5 {
                return true;
            }
        }
        
        // Check submenu if it's visible
        if self.show_open_with_menu {
            let sub_x = self.context_menu.map(|(cx, _)| cx).unwrap_or(0.0) + 120.0;
            let sub_y = self.context_menu.map(|(_, cy)| cy).unwrap_or(0.0) + 20.0;
            let sub_w = 150.0;
            let sub_h = (self.installed_apps.len() as f32 * 20.0).min(300.0);

            if mx >= sub_x && mx <= sub_x + sub_w && my >= sub_y && my <= sub_y + sub_h {
                return true;
            }
        }
        
        false
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
            let path = self.get_current_dir().join(&item.name);
            if item.prefix == "[DIR]" {
                self.input_text = path.to_string_lossy().into_owned();
                self.trigger_refresh();
            } else {
                self.execute_file(&path, None); // Use None for default behavior
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
        file_ops::copy_dir_recursive(src, dst)
    }

    fn cut_item(&mut self, selected_indices: Vec<usize>) {
        self.pending_paths = selected_indices.iter()
            .map(|&idx| self.get_current_dir().join(&self.file_list[idx].name))
            .collect();
    }

    fn copy_item(&mut self, selected_indices: Vec<usize>) {
        self.pending_paths = selected_indices.iter()
            .map(|&idx| self.get_current_dir().join(&self.file_list[idx].name))
            .collect();
    }

    fn paste_item(&mut self) {
        // Check if there are files in our internal buffer, NOT the system clipboard
        if !self.pending_paths.is_empty() {
            let dest = self.get_current_dir();
            // Pass the internal vector instead of the clipboard object
            let _ = file_ops::paste_item(&self.pending_paths, &dest);
            
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
            winit::event::WindowEvent::Resized(size) => {
                if let Some(pixels) = &mut self.pixels {
                    let _ = pixels.resize_surface(size.width, size.height);
                }
            }            
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                let (mx, my) = (position.x as f32, position.y as f32);
                // Call the new helper function
                self.handle_cursor_moved(mx, my);
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

                            if let Some((idx, win_x, win_y)) = self.properties_window {
                                let close_btn_x = win_x + 300.0 - 25.0; // matches the drawing logic
                                let close_btn_y = win_y + 5.0;
                                        
                                if mx >= close_btn_x && mx <= close_btn_x + 20.0 && my >= close_btn_y && my <= close_btn_y + 20.0 {
                                    self.properties_window = None;
                                    self.is_dragging_properties = false;
                                    return; // Exit early
                                }            

                                // Check if click is on the Title Bar (300px wide, 30px high)
                                if mx >= win_x && mx <= win_x + 300.0 && my >= win_y && my <= win_y + 30.0 {
                                    self.is_dragging_properties = true;
                                    self.properties_drag_offset = (mx - win_x, my - win_y);
                                    return; // Don't process other clicks while dragging
                                }
                            }

                            if my >= 45.0 && my <= 65.0 {
                                if mx >= 230.0 && mx <= 385.0 { self.sort_prop = scan_dir::SortProperty::Name; }
                                else if mx >= 390.0 && mx <= 585.0 { self.sort_prop = scan_dir::SortProperty::Size; }
                                else if mx >= 590.0 && mx <= 685.0 { self.sort_prop = scan_dir::SortProperty::MTime; }
                                else if mx >= 690.0 && mx <= 785.0 { self.sort_prop = scan_dir::SortProperty::ATime; }
                                else if mx >= 790.0 && mx <= 870.0 { self.sort_prop = scan_dir::SortProperty::CTime; }
                                
                                // Toggle order if clicking the same header
                                self.sort_order = if self.sort_order == scan_dir::SortOrder::Ascending { 
                                    scan_dir::SortOrder::Descending 
                                } else { 
                                    scan_dir::SortOrder::Ascending 
                                };
                                
                                self.trigger_refresh();
                                return;
                            }
                            if let Some((win_x, win_y)) = self.open_with_window {
                                // 1. Calculate the filtered list based on current search state
                                let filtered_apps: Vec<_> = self.installed_apps.iter()
                                    .filter(|a| a.name.to_lowercase().contains(&self.open_with_search.to_lowercase()))
                                    .collect();
                                
                                let rel_x = mx - win_x;
                                let rel_y = my - win_y;

                                // 2. Selection Logic (Detect which app row is clicked)
                                if rel_x >= 10.0 && rel_x <= 290.0 && rel_y >= 90.0 && rel_y <= 340.0 {
                                    println!("Clicked row: {}", ((rel_y - 90.0) / 25.0) as usize);
                                    let row_idx = ((rel_y - 90.0) / 25.0) as usize;
                                    if row_idx < filtered_apps.len() {
                                        self.open_with_selection = Some(row_idx);
                                        return; // Click consumed by selection
                                    }
                                    
                                }

                                // 3. Close Button
                                if mx >= win_x + 280.0 && mx <= win_x + 300.0 && my >= win_y && my <= win_y + 20.0 {
                                    self.open_with_window = None;
                                    return;
                                }

                                // 4. Open Button (Execution)
                                if mx >= win_x + 100.0 && mx <= win_x + 200.0 && my >= win_y + 360.0 && my <= win_y + 390.0 {
                                    if let Some(app_idx) = self.open_with_selection {
                                        if let Some(&file_idx) = self.selected_indices.iter().next() {
                                            let app_exec = &filtered_apps[app_idx].exec;
                                            let file_path = self.get_current_dir().join(&self.file_list[file_idx].name);
                                            
                                            // Execute via your existing helper
                                            self.execute_file(&file_path, Some(app_exec));
                                        }
                                    }
                                    self.open_with_window = None;
                                    self.open_with_selection = None;
                                    return;
                                }

                                // 5. Dragging (Title Bar)
                                if mx >= win_x && mx <= win_x + 300.0 && my >= win_y && my <= win_y + 30.0 {
                                    self.is_dragging_open_with = true;
                                    self.open_with_drag_offset = (rel_x, rel_y);
                                    return;
                                }

                                // 6. Window Boundary (Keep window open if clicked inside, close otherwise)
                                if mx >= win_x && mx <= win_x + 300.0 && my >= win_y && my <= win_y + 400.0 {
                                    return; 
                                } else {
                                    self.open_with_window = None;
                                    return;
                                }
                            }
                        
                            // 1. Check Context Menu
                            if let Some((cx, cy)) = self.context_menu {

                                let is_on_open = mx >= cx && mx <= cx + 120.0 && my >= cy && my <= cy + 20.0;
                                let is_on_open_with = mx >= cx && mx <= cx + 120.0 && my >= cy + 20.0 && my <= cy + 40.0;
                                let is_on_cut = mx >= cx && mx <= cx + 120.0 && my >= cy + 40.0 && my <= cy + 60.0;
                                let is_on_copy = mx >= cx && mx <= cx + 120.0 && my >= cy + 60.0 && my <= cy + 80.0;
                                let is_on_copy_path = mx >= cx && mx <= cx + 120.0 && my >= cy + 80.0 && my <= cy + 100.0;
                                let is_on_paste = mx >= cx && mx <= cx + 120.0 && my >= cy + 100.0 && my <= cy + 120.0;
                                let is_on_rename = mx >= cx && mx <= cx + 120.0 && my >= cy + 120.0 && my <= cy + 140.0;
                                let is_on_delete = mx >= cx && mx <= cx + 120.0 && my >= cy + 140.0 && my <= cy + 160.0;
                                let is_on_properties = mx >= cx && mx <= cx + 120.0 && my >= cy + 160.0 && my <= cy + 185.5;
                                

                                if is_on_open {
                                    if let Some(&first_sel) = self.selected_indices.iter().next() {
                                        self.open_item(first_sel);
                                    }
                                    self.context_menu = None;
                                    return;
                                } else if is_on_open_with {
                                    // Set the window position
                                    self.open_with_window = Some((100.0, 100.0)); 
                                    
                                    // CRITICAL: Close both menu layers
                                    self.context_menu = None; 
                                    self.show_open_with_menu = false; 
                                    
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
                                        let win_w = 300.0;
                                        let win_h = 200.0;
                                        
                                        // Clamp coordinates to keep window inside the application area
                                        let spawn_x = mx.clamp(0.0, WIDTH as f32 - win_w);
                                        let spawn_y = my.clamp(0.0, HEIGHT as f32 - win_h);
                                        
                                        self.properties_window = Some((first_sel, spawn_x, spawn_y));
                                        self.context_menu = None; // Hide menu after selection
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


                            if self.is_menu_open() {
                                // If we are clicking, check if we are clicking INSIDE the menu.
                                // If not, close the menu and return (don't click the buttons behind it).
                                if !self.is_inside_menu(mx, my) {
                                    self.context_menu = None;
                                    self.show_open_with_menu = false;
                                    return; // Consume the click, don't press buttons behind
                                }
                            } else if is_on_back {
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
                    // STATE: Released
                    // Reset ALL dragging-related flags to ensure the window "drops"
                    self.is_dragging_scrollbar = false;
                    self.is_dragging_properties = false;
                    self.is_dragging_open_with = false; // <--- Add this!
                    
                    // Clear selection state
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
                    // 1. CHECK MODAL FIRST: If open, handle it and EXIT immediately.
                    if self.open_with_window.is_some() {
                        match event.logical_key {
                            winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace) => { 
                                self.open_with_search.pop(); 
                            }
                            _ => if let Some(ref txt) = event.text {
                                self.open_with_search.push_str(&txt.to_string());
                            }
                        }
                        return; // EXIT: This prevents main path bar from seeing the key
                    }

                    // 2. MAIN APP INPUT: Only runs if open_with_window is None
                    let mut text_changed = false;
                    match event.logical_key {
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Backspace) => { 
                            self.input_text.pop(); 
                            text_changed = true; 
                        }
                        _ => if let Some(ref txt) = event.text {
                            for c in txt.chars() { 
                                if !c.is_control() { 
                                    self.input_text.push(c); 
                                    text_changed = true; 
                                } 
                            }
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

                    // Sorting logic
                    let sort_bg = [150, 0, 255, 255]; // Standard
                    let active_bg = [200, 50, 255, 255]; // Highlighted
                           
                    // Use fill for better performance than chunks_exact_mut if clearing to one color
                    frame.fill(0); // Clear to black
                    // Or your specific color:
                    for pixel in frame.chunks_exact_mut(4) { pixel.copy_from_slice(&[20, 20, 20, 255]); }

                    let (mx, my) = self.mouse_pos;
                    let btn_hover = |x: f32, y: f32, w: f32, h: f32| mx >= x && mx <= x+w && my >= y && my <= y+h;

                    // Header Buttons
                    // Back
                    let b_col = if btn_hover(10.0, 10.0, 75.0, 30.0) { [0, 150, 150, 255] } else { [0, 100, 100, 255] };
                    ui_components::draw_rect(frame, 10, 10, 75, 30, b_col);
                    self.world.draw_text(frame, "Back", 15, 30, 18.0, [255, 255, 255]);
                    
                    // Refresh
                    let r_col = if btn_hover(90.0, 10.0, 75.0, 30.0) { [0, 150, 150, 255] } else { [0, 100, 100, 255] };
                    ui_components::draw_rect(frame, 90, 10, 75, 30, r_col);
                    self.world.draw_text(frame, "Refresh", 95, 30, 18.0, [255, 255, 255]);

                    // Path Bar
                    ui_components::draw_rect(frame, 170, 10, 700, 30, [200, 100, 0, 255]); 
                    self.world.draw_text(frame, &self.input_text, 175, 32, 18.0, [0, 0, 0]); 

                    // Sidebar
                    ui_components::draw_rect(frame, 10, 70, 150, 460, [40, 40, 40, 255]); 

                    // Sort Options
                    //Self::draw_rect(frame, 170, 45, 55, 20, [150, 0, 255, 255]);
                    //Self::draw_rect(frame, 230, 45, 155, 20, [150, 0, 255, 255]);
                    //Self::draw_rect(frame, 390, 45, 195, 20, [150, 0, 255, 255]);
                    //Self::draw_rect(frame, 590, 45, 95, 20, [150, 0, 255, 255]);
                    //Self::draw_rect(frame, 690, 45, 95, 20, [150, 0, 255, 255]);
                    //Self::draw_rect(frame, 790, 45, 80, 20, [150, 0, 255, 255]);

                    // --- ADDED: Draw Column Labels ---
                    //self.world.draw_text(frame, "Type", 175, 60, 14.0, [255, 255, 255]);
                    //self.world.draw_text(frame, "Name", 235, 60, 14.0, [255, 255, 255]);
                    //self.world.draw_text(frame, "Size", 395, 60, 14.0, [255, 255, 255]);
                    //self.world.draw_text(frame, "MTime", 595, 60, 14.0, [255, 255, 255]);
                    //self.world.draw_text(frame, "ATime", 695, 60, 14.0, [255, 255, 255]);
                    //self.world.draw_text(frame, "CTime", 795, 60, 14.0, [255, 255, 255]);

                    // Unified Sort Header Drawing
                    let headers = [
                        (170, 55, "Type", scan_dir::SortProperty::Name), // Placeholder property for Type
                        (230, 155, "Name", scan_dir::SortProperty::Name),
                        (390, 195, "Size", scan_dir::SortProperty::Size),
                        (590, 95, "MTime", scan_dir::SortProperty::MTime),
                        (690, 95, "ATime", scan_dir::SortProperty::ATime),
                        (790, 80, "CTime", scan_dir::SortProperty::CTime),
                    ];
                    
                    for (x, w, label, prop) in headers {
                        let is_active = self.sort_prop == prop;
                        let col = if is_active { active_bg } else { sort_bg };
                        ui_components::draw_rect(frame, x, 45, w, 20, col);
                        self.world.draw_text(frame, label, (x + 5) as usize, 60, 14.0, [255, 255, 255]);
                    }
                    
                    // File List
                    for i in 0..VISIBLE_COUNT {
                        let actual_idx = self.scroll_index + i;
                        if actual_idx >= self.file_list.len() { break; }
                        let y_pos = 70 + (i as u32 * ROW_H);
                        let is_selected = self.selected_indices.contains(&actual_idx);
                        let is_hovered = mx >= LIST_X && mx <= LIST_X + LIST_W && my >= y_pos as f32 && my < (y_pos + 22) as f32;
                        
                        let col = if is_selected { [180, 0, 0, 255] } else if is_hovered { [60, 60, 60, 255] } else { [35, 35, 35, 255] };
                        ui_components::draw_rect(frame, LIST_X as u32, y_pos, LIST_W as u32, 22, col);
                        
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
                        ui_components::draw_rect(frame, rx as u32, ry as u32, rw as u32, rh as u32, [0, 120, 215, 80]);
                    }

                    // Context Menu (Drawn on top)
                    if let Some((cx, cy)) = self.context_menu {
                        let menu_w = 120;
                        let menu_h = 180; // CHANGE: Reduced from 480 to 150
                        let bg_color = [0, 0, 0, 255]; // CHANGE: Darker navy/slate color

                        // Draw main background
                        ui_components::draw_rect(frame, cx as u32, cy as u32, menu_w, menu_h, bg_color);

                        // Draw items with tighter spacing (e.g., 30px apart)
                        let items = ["Open", "Open With", "Cut", "Copy", "Copy Path", "Paste", "Rename", "Delete", "Properties"];
                        for (i, text) in items.iter().enumerate() {
                            let item_y = cy as u32 + (i as u32 * 20);
                            // Optional: Draw a hover effect or separator here
                            self.world.draw_text(frame, text, (cx as usize) + 10, (item_y + 20) as usize, 13.0, [255, 255, 255]);
                        }
                    }

                    // Properties Window (Drawn on top)
                    if let Some((idx, win_x, win_y)) = self.properties_window {
                        let win_w = 300.0;
                        let win_h = 200.0;

                        // Define title bar area
                        let title_bar_rect = (win_x, win_y, 300.0, 30.0);
                        
                        // 1. Draw Title Bar (Dark Grey)
                        ui_components::draw_rect(frame, win_x as u32, win_y as u32, win_w as u32, 30, [60, 60, 60, 255]);
                        
                        // 2. Draw Title Text
                        self.world.draw_text(frame, "Properties", (win_x + 10.0) as usize, (win_y + 20.0) as usize, 16.0, [255, 255, 255]);
                        
                        // 3. Draw Main Body (Darker grey)
                        ui_components::draw_rect(frame, win_x as u32, (win_y + 30.0) as u32, win_w as u32, (win_h - 30.0) as u32, [40, 40, 40, 255]);

                        // Close Button
                        let close_btn_x = win_x + win_w - 25.0;
                        let close_btn_y = win_y + 5.0;
                        ui_components::draw_rect(frame, close_btn_x as u32, close_btn_y as u32, 20, 20, [200, 50, 50, 255]);
                        self.world.draw_text(frame, "X", (close_btn_x + 5.0) as usize, (close_btn_y + 15.0) as usize, 14.0, [255, 255, 255]);
                        
                    ui_components::draw_rect(frame, 180, 470, 50, 50, [255, 0, 0, 255]);
                        // 4. Draw Item Info
                        if let Some(item) = self.file_list.get(idx) {
                            let text_y = win_y + 60.0;
                            self.world.draw_text(frame, &format!("Name: {}", item.name), (win_x + 10.0) as usize, text_y as usize, 14.0, [255, 255, 255]);
                            self.world.draw_text(frame, &format!("Size: {}", item.size), (win_x + 10.0) as usize, (text_y + 25.0) as usize, 14.0, [255, 255, 255]);
                                
                            // --- ADD: Permission Display (assuming item.permissions is available) ---
                            // Note: You may need to add a 'permissions' field to your FileDisplay struct in scan_dir.rs
                            self.world.draw_text(frame, &format!("Perms: {}", item.permissions), (win_x + 10.0) as usize, (text_y + 50.0) as usize, 14.0, [0, 255, 150]);
                        }
                    }
                                                
                    // Scrollbar
                    if self.file_list.len() > VISIBLE_COUNT {
                        let total = self.file_list.len() as f32;
                        let thumb_h = (VISIBLE_COUNT as f32 / total * TRACK_H).max(30.0);
                        let scroll_pct = self.scroll_index as f32 / (total - VISIBLE_COUNT as f32);
                        let thumb_y = TRACK_Y + (scroll_pct * (TRACK_H - thumb_h));
                        ui_components::draw_rect(frame, 880, TRACK_Y as u32, 12, TRACK_H as u32, [45, 45, 45, 255]); 
                        let thumb_col = if self.is_dragging_scrollbar { [200, 200, 200, 255] } else { [120, 120, 120, 255] };
                        ui_components::draw_rect(frame, 880, thumb_y as u32, 12, thumb_h as u32, thumb_col);
                    }

                    // Operations footer
                    ui_components::draw_rect(frame, 170, 460, 700, 70, [200, 100, 0, 255]);
                    // Operations buttons
                    self.world.draw_text(frame, "Opn", 185, 500, 20.0, [255, 255, 255]);
                    ui_components::draw_rect(frame, 240, 470, 50, 50, [0, 255, 0, 255]);
                    self.world.draw_text(frame, "Opw", 245, 500, 20.0, [255, 255, 255]);
                    ui_components::draw_rect(frame, 300, 470, 50, 50, [0, 0, 255, 255]);
                    self.world.draw_text(frame, "Cr", 310, 500, 20.0, [255, 255, 255]);
                    ui_components::draw_rect(frame, 360, 470, 50, 50, [255, 255, 0, 255]);
                    self.world.draw_text(frame, "", 370, 500, 20.0, [255, 255, 255]);

                    // Cut, Copy, CopyPath, Paste, Rename
                    // Cut 420
                    ui_components::draw_rect(frame, 420, 470, 50, 50, [0, 255, 255, 255]);
                    self.world.draw_text(frame, "Cut", 430, 500, 20.0, [255, 255, 255]);
                    // Copy 480
                    ui_components::draw_rect(frame, 480, 470, 50, 50, [255, 0, 255, 255]);
                    self.world.draw_text(frame, "CP", 490, 500, 20.0, [255, 255, 255]);
                    // Copy Path 540
                    ui_components::draw_rect(frame, 540, 470, 50, 50, [192, 192, 192, 255]);
                    self.world.draw_text(frame, "CPp", 550, 500, 20.0, [255, 255, 255]);
                    // Paste 600
                    ui_components::draw_rect(frame, 600, 470, 50, 50, [128, 0, 128, 255]);
                    self.world.draw_text(frame, "Pst", 610, 500, 20.0, [255, 255, 255]);
                    // Rename 660
                    ui_components::draw_rect(frame, 660, 470, 50, 50, [0, 128, 128, 255]);
                    self.world.draw_text(frame, "Ren", 670, 500, 20.0, [255, 255, 255]);
                    // Delete 720
                    ui_components::draw_rect(frame, 720, 470, 50, 50, [128, 128, 0, 255]);
                    self.world.draw_text(frame, "Del", 730, 500, 20.0, [255, 255, 255]);
                    // Properties
                    ui_components::draw_rect(frame, 780, 470, 50, 50, [0, 0, 0, 255]);
                    self.world.draw_text(frame, "Prp", 790, 500, 20.0, [255, 255, 255]);

                    // Open With window draw
                    if let Some((win_x, win_y)) = self.open_with_window {
                        let win_w = 300.0;
                        let win_h = 400.0;
                        
                        // 1. Background & Title
                        ui_components::draw_rect(frame, win_x as u32, win_y as u32, win_w as u32, 30, [60, 60, 60, 255]);
                        self.world.draw_text(frame, "Open With", (win_x + 10.0) as usize, (win_y + 20.0) as usize, 16.0, [255, 255, 255]);
                        ui_components::draw_rect(frame, win_x as u32, (win_y + 30.0) as u32, win_w as u32, (win_h - 30.0) as u32, [40, 40, 40, 255]);

                        // 2. Search Box
                        ui_components::draw_rect(frame, (win_x + 10.0) as u32, (win_y + 40.0) as u32, 280, 30, [20, 20, 20, 255]);
                        self.world.draw_text(frame, &self.open_with_search, (win_x + 15.0) as usize, (win_y + 62.0) as usize, 14.0, [200, 200, 200]);

                        // 3. Filtered List (One Column)
                        let filtered_apps: Vec<_> = self.installed_apps.iter()
                            .filter(|a| a.name.to_lowercase().contains(&self.open_with_search.to_lowercase()))
                            .collect();

                        for (i, app) in filtered_apps.iter().enumerate() {
                            if i >= 10 { break; } // Keep it to 10 visible
                            let y_pos = win_y + 90.0 + (i as f32 * 25.0);
                            
                            // Highlight selection
                            if self.open_with_selection == Some(i) {
                                ui_components::draw_rect(frame, (win_x + 10.0) as u32, y_pos as u32, 280, 20, [200, 50, 50, 255]);
                            }
                            self.world.draw_text(frame, &app.name, (win_x + 15.0) as usize, (y_pos + 18.0) as usize, 14.0, [255, 255, 255]);
                        }

                        // 4. Close Button (Top Right)
                        ui_components::draw_rect(frame, (win_x + 280.0) as u32, win_y as u32 + 5, 20, 20, [200, 50, 50, 255]);
                        self.world.draw_text(frame, "X", (win_x + 285.0) as usize, (win_y + 22.0) as usize, 14.0, [255, 255, 255]);

                        // 5. Open Button (Bottom Center)
                        ui_components::draw_rect(frame, (win_x + 100.0) as u32, (win_y + 360.0) as u32, 100, 30, [70, 130, 180, 255]);
                        self.world.draw_text(frame, "Open", (win_x + 135.0) as usize, (win_y + 382.0) as usize, 14.0, [255, 255, 255]);
                    }

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
        clipboard: arboard::Clipboard::new().ok(),
        pending_paths: Vec::new(),
        properties_window: None,
        properties_pos: None,
        is_dragging_properties: false,
        properties_drag_offset: (0.0, 0.0),
        sort_prop: scan_dir::SortProperty::Name,
        sort_order: scan_dir::SortOrder::Ascending,
        show_hidden: false, 
        fps_timer: std::time::Instant::now(), frame_count: 0,
        sys, cpu_usage: 0.0, ram_usage: 0, vram_usage: 0, pid,
        installed_apps: open_with::get_installed_apps(), // Load apps once at start
        show_open_with_menu: false,
        open_with_menu_pos: None,
        open_with_window: None,
        open_with_search: String::new(),
        open_with_selection: None,
        open_with_scroll: 0,
        is_dragging_open_with: false,
        open_with_drag_offset: (0.0, 0.0),
        search_query: String::new(),
    }; 
    event_loop.run_app(&mut app).unwrap(); 
}
