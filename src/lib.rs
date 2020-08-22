use std::io::{stdout, Stdout, Write};
use std::thread;
use std::mem::ManuallyDrop;
use std::{os::raw::c_char, sync::{Arc, Mutex, mpsc}, ffi::CStr};

use crossterm::{execute, terminal::{EnterAlternateScreen, enable_raw_mode}, event::DisableMouseCapture};
use serde;
use serde::*;
use tui::Terminal;
use tui::backend::CrosstermBackend;
use tui::widgets::{Widget, Block, Borders, List, Paragraph, ListItem};
use tui::{text::{Span, Spans}, layout::{Layout, Constraint, Direction}};

pub mod model;



pub struct TerminalHandler {
    pub terminal: ManuallyDrop<Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>>,
    pub tx: ManuallyDrop<crossbeam_channel::Sender<Message>>,
    pub bg_thread: ManuallyDrop<thread::JoinHandle<()>>,
}

pub enum Message {
    Packet(model::BacktestResultPacket),
    Log(String)
}

#[no_mangle]
extern "C" fn initialize() -> *mut TerminalHandler {
    enable_raw_mode().unwrap();

    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, DisableMouseCapture).unwrap();

    let backend = CrosstermBackend::new(stdout);
    let terminal = ManuallyDrop::new(Arc::new(Mutex::new(Terminal::new(backend).expect("Error creating terminal"))));
    let (tx, rx) = crossbeam_channel::unbounded();
    let tx = ManuallyDrop::new(tx);
    let rx = ManuallyDrop::new(rx);
    let term = terminal.clone();

    let bg_thread = ManuallyDrop::new(thread::spawn(move || {
        let mut logs = Vec::with_capacity(1024);

        loop {
            term.lock()
                .unwrap()
                .draw(|f| {
                    let size = f.size();
                    let block = Block::default()
                        .title("Logs")
                        .borders(Borders::ALL);

                    match rx.recv() {
                        Ok(val) => {
                            match val {
                                Message::Log(msg) => {
                                    &logs.push(ListItem::new(Span::raw(msg)));
                                    f.render_widget(List::new(logs.clone()), size);
                                },
                                _ => {}
                            }
                        },
                        Err(e) => println!("{:?}", e)
                    };
                })
                .unwrap();

            thread::sleep(std::time::Duration::from_millis(100));
        }
    }));

    let term_box = Box::new(TerminalHandler { 
        terminal,
        tx,
        bg_thread
    });

    Box::into_raw(term_box)
}

#[no_mangle]
unsafe extern "C" fn trace(handler: *mut TerminalHandler, raw_msg: *const c_char) {
    let raw_msg = CStr::from_ptr(raw_msg);
    let message = std::str::from_utf8(raw_msg.to_bytes()).unwrap();

    let terminal = Box::from_raw(handler);
    let log_msg = Message::Log(message.into());

    match terminal.tx.send(log_msg) {
        Ok(_) => {},
        Err(e) => println!("{:?}", e)
    }

    std::mem::forget(terminal);
}

#[no_mangle]
unsafe extern "C" fn error(handler: *mut TerminalHandler, message: *const c_char) {
    trace(handler, message);
}

#[no_mangle]
unsafe extern "C" fn free(handler: *mut TerminalHandler) {
    Box::from_raw(handler);
}