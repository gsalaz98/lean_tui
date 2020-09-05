

use std::{io::{stdout, Stdout, Read, Write}, sync::{Arc, Mutex}, thread};
use tui::{backend::CrosstermBackend, Terminal};
use crate::Message;
use crossterm::{execute, terminal::{EnterAlternateScreen, enable_raw_mode}, event::EnableMouseCapture};

/// Manages drawing of new terminal window and handling new input events
pub struct TerminalHandler {
    /// Crossterm terminal
    pub terminal: Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>,
    /// Channel we use to communicate with thread to avoid blocking the Lean thread
    pub tx: crossbeam_channel::Sender<Message>,
    /// Background thread manages and receives BacktestPackets from Lean
    pub bg_thread: thread::JoinHandle<()>,
}

impl TerminalHandler {
    pub fn default() -> Self {
        enable_raw_mode().unwrap();

        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture).unwrap();

        let backend = CrosstermBackend::new(stdout);
        let terminal = Arc::new(Mutex::new(Terminal::new(backend).expect("Error creating terminal")));
        let (tx, rx) = crossbeam_channel::unbounded();
        let bg_thread = TerminalHandler::start_draw(rx);

        Self {
            terminal,
            tx,
            bg_thread
        }
    }

    fn start_draw(rx: crossbeam_channel::Receiver<crate::Message>) {
        let bg_thread = thread::spawn(move || {
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
        });
    }
}