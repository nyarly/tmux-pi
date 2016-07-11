use std::sync::mpsc::{Receiver, Sender, channel};
pub trait TmuxCommand : Send{
  fn response_channel(&self) -> (Sender<Box<Response>>, Receiver<Box<Response>>) {
    channel()
  }
  fn build_response(&self) -> Box<Response>;
  fn wire_format(&self) -> &'static [u8];
}

pub trait Response : Send {
  fn consume(&mut self, text: &String);
}

pub mod info;
