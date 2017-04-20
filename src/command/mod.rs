use std::fmt::Debug;
use std::sync::mpsc::{Receiver, Sender, channel};

pub trait TmuxCommand: Send {
  fn response_channel(&self) -> (Sender<Box<Response>>, Receiver<Box<Response>>) {
    channel()
  }
  fn build_response(&self) -> Box<Response>;
  fn wire_format(&self) -> &'static [u8];
}

pub trait Response: Send + Debug {
  fn consume(&mut self, text: &String);
}

pub mod info;
