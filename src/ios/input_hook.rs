use crate::core::gui::Gui;
use egui::{PointerButton, Pos2};
use objc2::{msg_send, sel};
use objc2::runtime::{Class, Method, Object, Sel};
use objc2_foundation::CGPoint;
use once_cell::sync::OnceCell;
use std::ffi::c_void;

type TouchesBeganFn = unsafe extern "C" fn(this: *mut Object, sel: Sel, touches: *mut Object, event: *mut Object);

static ORIG_TOUCHES_BEGAN: OnceCell<TouchesBeganFn> = OnceCell::new();

unsafe extern "C" fn on_touches_began(
    this: *mut Object,
    sel: Sel,
    touches: *mut Object,
    event: *mut Object,
) {
    if let Some(gui_mutex) = Gui::instance() {
        let mut gui = gui_mutex.lock().unwrap();

        let all_touches: *mut Object = msg_send![touches, allObjects];
        let count: usize = msg_send![all_touches, count];

        if count == 3 {
            let touch: *mut Object = msg_send![all_touches, objectAtIndex: 0];
            let phase: i64 = msg_send![touch, phase];

            if phase == 0 {
                info!("3-finger tap detected. Toggling GUI.");
                gui.toggle_menu();
                return; 
            }
        }

        if gui.is_consuming_input() {

            for i in 0..count {
                let touch: *mut Object = msg_send![all_touches, objectAtIndex: i];

                let window: *mut Object = msg_send![touch, window];
                if window.is_null() {
                    continue;
                }

                let location: CGPoint = msg_send![touch, locationInView: window];
                let pos = Pos2::new(location.x as f32, location.y as f32);

                let phase: i64 = msg_send![touch, phase];

                if i == 0 {
                    match phase {
                        0 => {
                            gui.input.events.push(egui::Event::PointerButton {
                                pos,
                                button: PointerButton::Primary,
                                pressed: true,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                        1 => {
                            gui.input.events.push(egui::Event::PointerMoved(pos));
                        }
                        3 | 4 => {
                            gui.input.events.push(egui::Event::PointerButton {
                                pos,
                                button: PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            });
                            gui.input.events.push(egui::Event::PointerGone);
                        }
                        _ => {}
                    }
                }
            }

            return;
        }
    }

    if let Some(orig) = ORIG_TOUCHES_BEGAN.get() {
        orig(this, sel, touches, event);
    }
}

pub fn init() {
    info!("Initializing iOS input hook...");

    unsafe {
        let class = match Class::get("UIView") {
            Some(c) => c,
            None => {
                error!("Failed to find UIView class. Input hooking will not work.");
                return;
            }
        };

        let sel = sel!(touchesBegan:withEvent:);

        let method: *const Method = match class.instance_method(sel) {
            Some(m) => m,
            None => {
                error!("Failed to find method touchesBegan:withEvent: on UIView.");
                return;
            }
        };

        let target_fn_addr: usize = std::mem::transmute((*method).implementation());

        let hachimi = crate::core::Hachimi::instance();

        match hachimi.interceptor.hook(target_fn_addr, on_touches_began as usize) {
            Ok(trampoline) => {
                ORIG_TOUCHES_BEGAN.set(std::mem::transmute(trampoline)).unwrap();
                info!("Successfully hooked UIView touchesBegan:withEvent:");
            }
            Err(e) => {
                error!("Failed to hook touchesBegan:withEvent:: {}", e);
            }
        }
    }
}