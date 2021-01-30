pub mod model;
pub mod terminal;

use std::io::{stdout, Stdout, Write};
use std::thread;
use std::mem::ManuallyDrop;
use std::{
    os::raw::c_char, 
    sync::{Arc, Mutex}, 
    ffi::CStr
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute, 
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, enable_raw_mode, disable_raw_mode}, 
};
use tui::Terminal;
use tui::backend::CrosstermBackend;
use tui::widgets::{Block, Borders, List, ListItem, Chart, Dataset, GraphType, Axis};
use tui::{
    text::Span, 
    layout::{Layout, Constraint, Direction}, 
    style::{Style, Color}, symbols::Marker
};

use crate::terminal::terminal_handler::TerminalHandler;

pub enum Message {
    Packet(model::BacktestResultPacket),
    Log(String, bool),
    Stop
}

#[no_mangle]
extern "C" fn initialize() -> *mut TerminalHandler {
    let mut terminal_handler = TerminalHandler::default(); 
    terminal_handler.start();

    Box::into_raw(Box::new(terminal_handler))
}

#[no_mangle]
unsafe extern "C" fn update(handler: *mut TerminalHandler, raw_msg: *const c_char) {
    let raw_msg = CStr::from_ptr(raw_msg);
    let message = std::str::from_utf8(raw_msg.to_bytes()).unwrap();

    let terminal = Box::from_raw(handler);
    let de_packet = serde_json::from_str::<model::BacktestResultPacket>(&message);
    
    match de_packet {
        Ok(packet) => terminal.tx.send(Message::Packet(packet)).unwrap(),
        Err(err) => {
            std::fs::write("bterror.log", format!("{:?}", err)).unwrap();
            std::fs::write("btresultpacket.json", message).unwrap();
        }
    };

    std::mem::forget(terminal);
}

#[no_mangle]
unsafe extern "C" fn trace(handler: *mut TerminalHandler, raw_msg: *const c_char) {
    let raw_msg = CStr::from_ptr(raw_msg);
    let message = std::str::from_utf8(raw_msg.to_bytes()).unwrap();

    let terminal = Box::from_raw(handler);
    let log_msg = Message::Log(message.into(), false);

    match terminal.tx.send(log_msg) {
        Ok(_) => {},
        Err(e) => println!("{:?}", e)
    }

    std::mem::forget(terminal);
}

#[no_mangle]
unsafe extern "C" fn error(handler: *mut TerminalHandler, raw_msg: *const c_char) {
    let raw_msg = CStr::from_ptr(raw_msg);
    let message = std::str::from_utf8(raw_msg.to_bytes()).unwrap();

    let terminal = Box::from_raw(handler);
    let log_msg = Message::Log(message.into(), true);

    match terminal.tx.send(log_msg) {
        Ok(_) => {},
        Err(e) => println!("{:?}", e)
    }

    std::mem::forget(terminal);
}

#[no_mangle]
unsafe extern "C" fn free(handler: *mut TerminalHandler) {
    let terminal = Box::from_raw(handler);

    disable_raw_mode().unwrap();
    terminal.tx.send(Message::Stop).unwrap();

    execute!((*terminal).terminal.lock().unwrap().backend_mut(), LeaveAlternateScreen, DisableMouseCapture)
        .unwrap();
}