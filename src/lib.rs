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

pub mod model;


pub struct TerminalHandler {
    pub terminal: ManuallyDrop<Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>>,
    pub tx: ManuallyDrop<crossbeam_channel::Sender<Message>>,
    pub bg_thread: ManuallyDrop<thread::JoinHandle<()>>,
}

pub enum Message {
    Packet(model::BacktestResultPacket),
    Log(String, bool),
    Stop
}

#[no_mangle]
extern "C" fn initialize() -> *mut TerminalHandler {
    enable_raw_mode().unwrap();

    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).unwrap();

    let backend = CrosstermBackend::new(stdout);
    let terminal = ManuallyDrop::new(Arc::new(Mutex::new(Terminal::new(backend).expect("Error creating terminal"))));
    let (tx, rx) = crossbeam_channel::unbounded();
    let tx = ManuallyDrop::new(tx);
    let rx = ManuallyDrop::new(rx);
    let term = terminal.clone();

    let bg_thread = ManuallyDrop::new(thread::spawn(move || {
        let mut logs = Vec::with_capacity(1024);
        let mut equity_data: Vec<(f64, f64)> = vec![];
        let mut order_times = vec![];
        let mut order_types= vec![];
        let mut order_sides = vec![];
        let mut order_qty = vec![];
        let mut order_symbol = vec![];

        loop {
            let mut finished = false;

            term.lock()
                .unwrap()
                .draw(|f| {
                    let hchunk = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Percentage(75),
                            Constraint::Percentage(25)
                        ].as_ref())
                        .split(f.size());

                    match rx.recv() {
                        Ok(val) => {
                            match val {
                                Message::Log(msg, error) => {
                                    let log_style = Style::default()
                                        .fg(if error { Color::Red } else { Color::Reset });

                                    for line in msg.lines() {
                                        let log_line = ListItem::new(Span::styled(line.to_string(), log_style));
                                        &logs.push(log_line);
                                    }
                                },
                                Message::Packet(packet) => {
                                    if packet.Results.Charts.is_some() {
                                        let packet_charts = packet.Results.Charts.unwrap();
                                        match packet_charts.get("Strategy Equity").map(|v| v.Series.get("Equity").unwrap()) {
                                            Some(points) => {
                                                let new_points = points.Values.clone()
                                                    .into_iter()
                                                    .map(|xy| (xy.x, xy.y))
                                                    .filter(|(_, y)| y > &0f64)
                                                    .collect::<Vec<(f64, f64)>>();

                                                for point in new_points {
                                                    equity_data.push(point);
                                                }

                                                equity_data.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                                                equity_data.dedup();
                                            },
                                            None => {}
                                        };
                                    }

                                    if packet.Results.Orders.is_some() {
                                        let mut current_orders = packet.Results.Orders.unwrap()
                                            .into_iter()
                                            .collect::<Vec<(String, model::Order)>>();

                                        // Sort the keys of the HashMap so that our orders are aligned exactly as they came in
                                        current_orders.sort_by(|x, y| x.0.parse::<u64>().unwrap().cmp(&y.0.parse::<u64>().unwrap()));

                                        order_times.clear();
                                        order_types.clear();
                                        order_sides.clear();
                                        order_qty.clear();
                                        order_symbol.clear();

                                        for (_, order) in current_orders {
                                            let (order_time, order_type, direction, quantity, symbol) = order.into_spans();

                                            order_times.push(order_time);
                                            order_types.push(order_type);
                                            order_sides.push(direction);
                                            order_qty.push(quantity);
                                            order_symbol.push(symbol);
                                        }   
                                    }
                                },
                                Message::Stop => finished = true
                            }
                        },
                        Err(_) => {}
                    };

                    let graph_log_chunk= Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Percentage(70),
                            Constraint::Percentage(30)
                        ].as_ref())
                        .split(hchunk[0]);

                    let order_chunk = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Percentage(35),
                            Constraint::Percentage(35),
                            Constraint::Percentage(30)
                        ].as_ref())
                        .split(hchunk[1]);

                    let orders_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Length(27),
                            Constraint::Length(8),
                            Constraint::Length(11),
                            Constraint::Length(8),
                            Constraint::Length(11)
                        ].as_ref())
                        .split(order_chunk[2]);

                    let graph_block = Block::default()
                        .title("Backtest Performance")
                        .borders(Borders::ALL);

                    let orders_time_block = Block::default()
                        .title("Time")
                        .borders(Borders::ALL);
                    let orders_type_block = Block::default()
                        .title("Type")
                        .borders(Borders::ALL);
                    let orders_direction_block = Block::default()
                        .title("Direction")
                        .borders(Borders::ALL);
                    let orders_symbol_block = Block::default()
                        .title("Symbol")
                        .borders(Borders::ALL);
                    let orders_qty_block = Block::default()
                        .title("Quantity")
                        .borders(Borders::ALL);

                    let log_block = Block::default()
                        .title("Algorithm Logs")
                        .borders(Borders::ALL);

                    let widget_orders_time = List::new(order_times.clone().into_iter().rev().take(orders_chunks[0].height as usize - 2).rev().map(|s| ListItem::new(s)).collect::<Vec<ListItem>>()).block(orders_time_block);
                    let widget_orders_type = List::new(order_types.clone().into_iter().rev().take(orders_chunks[1].height as usize - 2).rev().map(|s| ListItem::new(s)).collect::<Vec<ListItem>>()).block(orders_type_block);
                    let widget_orders_direction = List::new(order_sides.clone().into_iter().rev().take(orders_chunks[2].height as usize - 2).rev().map(|s| ListItem::new(s)).collect::<Vec<ListItem>>()).block(orders_direction_block);
                    let widget_orders_symbol = List::new(order_symbol.clone().into_iter().rev().take(orders_chunks[3].height as usize - 2).rev().map(|s| ListItem::new(s)).collect::<Vec<ListItem>>()).block(orders_symbol_block);
                    let widget_orders_qty = List::new(order_qty.clone().into_iter().rev().take(orders_chunks[4].height as usize - 2).rev().map(|s| ListItem::new(s)).collect::<Vec<ListItem>>()).block(orders_qty_block);
                    
                    let last_logs = logs
                        .clone()
                        .into_iter()
                        .rev()
                        .take(graph_log_chunk[1].height as usize - 2)
                        .rev()
                        .collect::<Vec<ListItem>>();
                        
                    let draw_graph = equity_data.len() != 0;

                    let log_widget= List::new(last_logs)
                        .block(log_block);

                    if draw_graph { 
                        let x_axis_bounds = [equity_data[0].0, equity_data[equity_data.len() - 1].0];
                        let y_axis_bounds = [
                            equity_data.iter().map(|(_, y)| y).fold(f64::INFINITY, |a, &b| a.min(b)),
                            equity_data.iter().map(|(_, y)| y).fold(0f64, |a, &b| a.max(b))
                        ];

                        let graph_widget = Chart::new(vec![
                        Dataset::default()
                            .name("Equity Curve")
                            .graph_type(GraphType::Line)
                            .marker(Marker::Dot)
                            .style(Style::default().fg(Color::White))
                            .data(&equity_data)])
                        .block(graph_block)
                        .x_axis(Axis::default()
                            .title("Time")
                            .bounds(x_axis_bounds))
                        .y_axis(Axis::default()
                            .title("Equity")
                            .bounds(y_axis_bounds));

                        f.render_widget(graph_widget, graph_log_chunk[0]);
                    }
                    else {
                        f.render_widget(graph_block, graph_log_chunk[0]);
                    }

                    f.render_widget(log_widget, graph_log_chunk[1]);

                    f.render_widget(widget_orders_time, orders_chunks[0]);
                    f.render_widget(widget_orders_type, orders_chunks[1]);
                    f.render_widget(widget_orders_direction, orders_chunks[2]);
                    f.render_widget(widget_orders_symbol, orders_chunks[3]);
                    f.render_widget(widget_orders_qty, orders_chunks[4]);

            })
            .unwrap();

            if finished {
                break;
            }
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

    execute!(*(*terminal.terminal).lock().unwrap().backend_mut(), LeaveAlternateScreen, DisableMouseCapture)
        .unwrap();
}