struct Cmd;

struct Response {
  content: String
}

use super::TmuxCommand;
impl TmuxCommand for Cmd {
  fn build_response(&self) -> Box<super::Response> {
    Box::new(Response::new())
  }
  fn wire_format(&self) -> &'static [u8] {
    b"info"
  }
}

impl super::Response for Response {
  fn consume(&mut self, text: &String) {
    self.content = text.clone()
  }
}

impl Response {
  fn new() -> Response {
    Response{
      content: String::from("")
    }
  }
}
