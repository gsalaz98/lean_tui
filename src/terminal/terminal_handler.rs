

use std::{io::{stdout, Stdout, Read, Write}, sync::{Arc, Mutex}, thread};
use tui::{Terminal, backend::CrosstermBackend, layout::{Constraint, Direction, Layout}, style::{Color, Style}, symbols::Marker, text::Span, widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, List, ListItem, Widget}};
use crate::Message;
use crossterm::{event::EnableMouseCapture, execute, terminal::{EnterAlternateScreen, enable_raw_mode}};


/// In charge of handling rendering to the terminal frame
pub trait TerminalRenderer {
    /// Renders the section of the terminal
    fn render(&self, frame: &mut tui::Frame<CrosstermBackend<Stdout>>);
}

/// Manages drawing of new terminal window and handling new input events
pub struct TerminalHandler {
    /// Crossterm terminal
    pub terminal: Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>,
    /// Channel we use to communicate with thread to avoid blocking the Lean thread
    pub tx: crossbeam_channel::Sender<Message>,
    /// Channel we use to receive data from the thread
    pub receiver: crossbeam_channel::Receiver<Message>,
    /// Background thread manages and receives BacktestPackets from Lean
    pub bg_thread: Option<thread::JoinHandle<()>>,
}

#[derive(Clone, Debug, Default)]
pub struct TerminalData<'a> {
    logs: Vec<ListItem<'a>>,
    equity: Vec<(f64, f64)>,

    order_time: Vec<Span<'a>>,
    order_type: Vec<Span<'a>>,
    order_sides: Vec<Span<'a>>,
    order_qty: Vec<Span<'a>>,
    order_symbol: Vec<Span<'a>>,
}

pub struct Term<'a> {
    pub left: LeftTerminalChunks<'a>,
    pub right: RightTerminalChunk<'a>,
    data: &'a TerminalData<'a>
}

pub struct LeftTerminalChunks<'a> {
    graph: tui::layout::Rect,
    logs: tui::layout::Rect,
    data: &'a TerminalData<'a>
}

pub struct RightTerminalChunk<'a> {
    orders: OrdersChunk,
    performance: tui::layout::Rect,
    metrics: tui::layout::Rect,
    data: &'a TerminalData<'a>
}

pub struct OrdersChunk {
    order_time: tui::layout::Rect,
    order_type: tui::layout::Rect,
    order_direction: tui::layout::Rect,
    order_symbol: tui::layout::Rect,
    order_quantity: tui::layout::Rect
}


impl Default for TerminalHandler {
    fn default() -> Self {
        enable_raw_mode().unwrap();

        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture).unwrap();

        let backend = CrosstermBackend::new(stdout);
        let terminal = Arc::new(Mutex::new(Terminal::new(backend).expect("Error creating terminal")));
        let (tx, rx) = crossbeam_channel::unbounded();
        
        Self {
            terminal,
            tx,
            receiver: rx,
            bg_thread: None
        }
    }
}

impl TerminalHandler {
    pub fn start(&mut self) {
        let terminal = self.terminal.clone();
        let rx = self.receiver.clone();

        self.bg_thread = Some(thread::spawn(move || {
            let mut terminal_data = TerminalData::default();
            let mut finished = false;

            while !finished {
                terminal
                    .lock()
                    .unwrap()
                    .draw(|f| {
                        finished = terminal_data.handle_data(&rx);
                        if finished {
                            return
                        }

                        Term::render(f, &terminal_data);
                })
                .unwrap();

                if finished {
                    break;
                }
            }
        }));
    }
}

impl<'a> Term<'a> {
    pub fn render(frame: &mut tui::Frame<CrosstermBackend<Stdout>>, terminal_data: &'a TerminalData) {
        let hchunk = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(75),
                Constraint::Percentage(25)
            ].as_ref())
            .split(frame.size());

        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(70),
                Constraint::Percentage(30)
            ].as_ref())
            .split(hchunk[0]);

        let right = Layout::default()
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
            .split(right[0]);

        let renderer = Self {
            left: LeftTerminalChunks::new(left, &terminal_data),
            right: RightTerminalChunk::new(right, orders_chunks, &terminal_data),
            data: terminal_data
        };

        renderer.left.render(frame);
        renderer.right.render(frame);
    }

    pub fn render_graph(&self, frame: &mut tui::Frame<CrosstermBackend<Stdout>>) {
        let graph_block = Block::default()
            .title("Backtest Performance")
            .borders(Borders::ALL);

        if self.data.equity.len() != 0 {
            frame.render_widget(graph_block, self.left.graph);
            return
        }

        let equity = &self.data.equity;
        let x_axis_bounds = [equity[0].0, equity[equity.len() - 1].0];
        let y_axis_bounds = [
            equity.iter().map(|(_, y)| y).fold(f64::INFINITY, |a, &b| a.min(b)),
            equity.iter().map(|(_, y)| y).fold(0f64, |a, &b| a.max(b))
        ];

        let graph_widget = Chart::new(vec![
        Dataset::default()
            .name("Equity Curve")
            .graph_type(GraphType::Line)
            .marker(Marker::Dot)
            .style(Style::default().fg(Color::White))
            .data(&equity)])
        .block(graph_block)
        .x_axis(Axis::default()
            .title("Time")
            .bounds(x_axis_bounds))
        .y_axis(Axis::default()
            .title("Equity")
            .bounds(y_axis_bounds));

        frame.render_widget(graph_widget, self.left.graph);
    }
}


impl<'a> TerminalData<'a> {
    pub fn handle_data(&mut self, rx: &crossbeam_channel::Receiver<Message>) -> bool {
        match rx.recv() {
            Ok(val) => {
                match val {
                    Message::Log(msg, error) => self.log(msg, error),
                    Message::Packet(packet) => self.packet(packet),
                    Message::Stop => return true
                }
            },
            Err(_) => {}
        };

        false
    }

    fn log(&mut self, msg: String, error: bool) {
        let log_style = Style::default()
            .fg(if error { Color::Red } else { Color::Reset });

        for line in msg.lines() {
            let log_line = ListItem::new(Span::styled(line.to_string(), log_style));
            &self.logs.push(log_line);
        }
    }

    fn packet(&mut self, packet: crate::model::BacktestResultPacket) {
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
                        &self.equity.push(point);
                    }

                    &self.equity.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                    &self.equity.dedup();
                },
                None => {}
            };
        }

        if packet.Results.Orders.is_some() {
            let mut current_orders = packet.Results.Orders.unwrap()
                .into_iter()
                .collect::<Vec<(String, crate::model::Order)>>();

            // Sort the keys of the HashMap so that our orders are aligned exactly as they came in
            current_orders.sort_by(|x, y| x.0.parse::<u64>().unwrap().cmp(&y.0.parse::<u64>().unwrap()));

            &self.order_time.clear();
            &self.order_type.clear();
            &self.order_sides.clear();
            &self.order_qty.clear();
            &self.order_symbol.clear();

            for (_, order) in current_orders {
                let (order_time, order_type, direction, quantity, symbol) = order.into_spans();

                &self.order_time.push(order_time);
                &self.order_type.push(order_type);
                &self.order_sides.push(direction);
                &self.order_qty.push(quantity);
                &self.order_symbol.push(symbol);
            }   
        }
    }
}

impl<'a> LeftTerminalChunks<'a> {
    pub fn new(chunks: Vec<tui::layout::Rect>, data: &'a TerminalData) -> Self {
        Self {
            graph: chunks[0],
            logs: chunks[1],
            data
        }
    }
}

impl<'a> RightTerminalChunk<'a> {
    pub fn new(chunks: Vec<tui::layout::Rect>, order_chunk: Vec<tui::layout::Rect>, data: &'a TerminalData) -> Self {
        Self {
            orders: OrdersChunk::new(order_chunk),
            performance: chunks[1],
            metrics: chunks[2],
            data
        }
    }
}

impl OrdersChunk {
    pub fn new(chunk: Vec<tui::layout::Rect>) -> Self {
        Self {
            order_time: chunk[0],
            order_type: chunk[1],
            order_direction: chunk[2],
            order_symbol: chunk[3], 
            order_quantity: chunk[4]
        }
    }
}

impl<'a> TerminalRenderer for LeftTerminalChunks<'a> {
    fn render(&self, frame: &mut tui::Frame<CrosstermBackend<Stdout>>) {
        let log_block = Block::default()
            .title("Algorithm Logs")
            .borders(Borders::ALL);

        let logs = self.data.logs
            .iter()
            .rev()
            .take(self.logs.height as usize - 2)
            .rev()
            .map(|v| v.clone())
            .collect::<Vec<ListItem>>();

        let log_widget = List::new(logs)
            .block(log_block);

        frame.render_widget(log_widget, self.logs);
    }
}

impl<'a> TerminalRenderer for RightTerminalChunk<'a> {
    fn render(&self, frame: &mut tui::Frame<CrosstermBackend<Stdout>>) {
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

        let widget_orders_time = List::new(self.data.order_time.iter().rev().take(self.orders.order_time.height as usize - 2).rev().map(|s| ListItem::new(s.clone())).collect::<Vec<ListItem>>()).block(orders_time_block);
        let widget_orders_type = List::new(self.data.order_type.iter().rev().take(self.orders.order_type.height as usize - 2).rev().map(|s| ListItem::new(s.clone())).collect::<Vec<ListItem>>()).block(orders_type_block);
        let widget_orders_direction = List::new(self.data.order_sides.iter().rev().take(self.orders.order_direction.height as usize - 2).rev().map(|s| ListItem::new(s.clone())).collect::<Vec<ListItem>>()).block(orders_direction_block);
        let widget_orders_symbol = List::new(self.data.order_symbol.iter().rev().take(self.orders.order_symbol.height as usize - 2).rev().map(|s| ListItem::new(s.clone())).collect::<Vec<ListItem>>()).block(orders_symbol_block);
        let widget_orders_qty = List::new(self.data.order_qty.iter().rev().take(self.orders.order_quantity.height as usize - 2).rev().map(|s| ListItem::new(s.clone())).collect::<Vec<ListItem>>()).block(orders_qty_block);

        frame.render_widget(widget_orders_time, self.orders.order_time);
        frame.render_widget(widget_orders_type, self.orders.order_type);
        frame.render_widget(widget_orders_direction, self.orders.order_direction);
        frame.render_widget(widget_orders_symbol, self.orders.order_symbol);
        frame.render_widget(widget_orders_qty, self.orders.order_quantity);
    }
}