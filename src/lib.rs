use std::thread::{self, JoinHandle};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

extern crate regex;

struct TmuxControl {
  //tmux: Child,
  reader: JoinHandle<()>,
  writer: JoinHandle<()>,
  commands: Sender<Pair>,
}

struct Pair(Box<TmuxCommand>, Sender<Box<Response>>);
struct SeqRs(u64, Box<Response>, Sender<Box<Response>>);

impl TmuxControl {
  fn start() -> TmuxControl {
    let child = Command::new("tmux")
                .arg("-C")
                .env_remove("TMUX")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
    let (responses, send) = channel();
    let (tell, commands) = channel();
    let stdin = child.stdin.expect("didn't open a stdin pipe?!");
    let stdout = child.stdout.expect("didn't open a stdout pipe?!");
    let writer = thread::spawn(move || write_command_stream(commands, responses, stdin));
    let reader = thread::spawn(move || read_command_stream(stdout, send));

    TmuxControl {
      // tmux: child, //how do we kill the child tmux process?
      reader: reader,
      writer: writer,
      commands: tell,
    }
  }

  fn transact(&self, cmd: Box<TmuxCommand>) -> Receiver<Box<Response>> {
    let (tx, rx) = cmd.response_channel();
    self.commands.send(Pair(cmd, tx));
    rx
  }
}

fn write_command_stream(commands: Receiver<Pair>, results: Sender<SeqRs>, mut stdin: ChildStdin) {
  let message: String;
  let mut seq = 0;
  loop {
    let Pair(command, tx) = commands.recv().unwrap();
    seq += 1;
    results.send(SeqRs(seq, command.build_response(), tx));
    let res = stdin.write(command.wire_format());
    res.unwrap();
  }
}

fn read_command_stream(stdout: ChildStdout, send_channels: Receiver<SeqRs>) {
  let mut csr = CommandStreamReader::new();
  csr.read(stdout, send_channels)
}


trait TmuxCommand : Send{
  fn response_channel(&self) -> (Sender<Box<Response>>, Receiver<Box<Response>>) {
    channel()
  }
  fn build_response(&self) -> Box<Response>;
  fn wire_format(&self) -> &'static [u8];
}

trait Response : Send {
  fn consume(&mut self, text: &String);
}

struct InfoCmd;

struct InfoResponse {
  content: String
}

impl TmuxCommand for InfoCmd {
  fn build_response(&self) -> Box<Response> {
    Box::new(InfoResponse::new())
  }
  fn wire_format(&self) -> &'static [u8] {
    b"info"
  }
}

impl Response for InfoResponse {
  fn consume(&mut self, text: &String) {
    self.content = text.clone()
  }
}

impl InfoResponse {
  fn new() -> InfoResponse {
    InfoResponse{
      content: String::from("")
    }
  }
}

struct RawResponse {
  num: u64,
  output: String,
}

use regex::{Captures, Regex};
use std::io::Write;

struct CommandStreamReader {
  buffer: [u8; 4096],
  start: usize,
  end: usize,
  max: usize,
  half: usize,
  maybe_output: Option<String>,
  response_mode: bool,
  max_end_length: usize,
  results: Vec<RawResponse>,
  stanza_re: Regex,
  stanza_end_re: Regex,
  arg_re: Regex,
}

type StreamBuffer<'a> = &'a [u8];

use std::{str, u64};
use std::str::FromStr;
use std::io::Read;
use std::sync::mpsc::TryRecvError;
impl  CommandStreamReader  {
  fn new() -> CommandStreamReader {
    let max = 4096;
    CommandStreamReader{
      buffer: [0; 4096],
      start: 0,
      end: 0,
      max: max,
      half : max / 2,
      response_mode : false,
      maybe_output: None,
      results : Vec::new(),

      stanza_re : Regex::new("(?m)^%(begin|exit|layout-change|output|session-changed|\
      session-renamed|sessions-changed|unlinked-window-add|window-add|window-close|\
      window-renamed)").unwrap(),
      stanza_end_re : Regex::new("(?m)^%(end|error)").unwrap(),
      max_end_length : 6,

      arg_re : Regex::new(r"^(\s+(?<first>\S+))?(\s+(?<second>\S+))?(\s+(?<third>\S+))?\s*\n").unwrap(),
    }
  }

  fn read(&mut self, mut stdout: ChildStdout, send_channels: Receiver<SeqRs>) {
    loop {
      match stdout.read(&mut self.buffer[self.end..(self.max - 1)]) {
        Ok(num) => self.end += num,
        Err(err) => panic!(err),
      }

      match self.maybe_output {
        None => self.capture_notification(),
        Some(ref block) => self.capture_output_block(block),
      }

      while self.results.len() > 0 {
        match send_channels.try_recv() {
          Err(TryRecvError::Empty) => break,
          Err(_) => return,
          Ok(SeqRs(num, ref resp, ref tx)) => {
            let pos = self.results.iter()
                             .position(|&r| r.num == num)
                             .expect("received response ahead of command");
            let res = self.results.swap_remove(pos);
            resp.consume(&(res.output));
            tx.send(*resp).unwrap_or(())
          }
        }
      }

      if self.start > self.half {
        self.buffer[..(self.end - self.start)].clone_from_slice(&self.buffer[self.start..self.end]);
        self.end = self.end - self.start;
        self.start = 0
      }
    }
  }

  fn advance_buffer(&mut self, captures: &Captures) {
    let (b, e) = captures.pos(0).unwrap();
    self.start += e;
  }

  fn buffer_prefix(&self, end: usize) -> StreamBuffer {
    &self.buffer[self.start..end]
  }

  fn valid_str(&self, end: usize) -> &str {
    match str::from_utf8(self.buffer_prefix(end)) {
      Ok(s) => s,
      Err(e) => str::from_utf8(&self.buffer_prefix(end)[..e.valid_up_to()]).unwrap()
    }
  }

  fn current_buffer(&self) -> StreamBuffer {
    self.buffer_prefix(self.end)
  }

  fn current_str(&self) -> &str {
    self.valid_str(self.end)
  }

  fn get_args(&mut self) -> (Option<&str>, Option<&str>, Option<&str>) {
    let captures = self.arg_re.captures(self.current_str()).unwrap();
    self.advance_buffer(&captures);
    (captures.at(1), captures.at(2), captures.at(3))
  }

  fn capture_notification(&mut self) {
    match self.stanza_re.captures(self.current_str()) {
      None => (),
      Some(captures) => {
        self.advance_buffer(&captures);

        match captures.at(1).unwrap() {
          "begin" => {
            self.get_args();
            self.maybe_output = Some(String::from(""))
          }
          "exit" => (),
          "layout-change" => (),
          "output" => (),
          "session-changed" => (),
          "session-renamed" => (),
          "sessions-changed" => (),
          "unlinked-window-add" => (),
          "window-add" => (),
          "window-close" => (),
          "window-renamed" => (),
          _ => (), //maybe a warning or something...
        }
      }
    }
  }

  fn capture_output_block(&mut self, block: &String) {
    let limit: usize;
    match self.stanza_end_re.captures(self.current_str()) {
      None => {
        limit = self.end - (self.max_end_length);
        block.push_str(self.current_str());
        self.start = limit;
      }
      Some(capture) => {
        let (limit, finish) = capture.pos(0).expect("the zero capture should always exist, right?");
        let (timestamp, number, flags) = self.get_args();
        block.push_str(self.valid_str(limit));
        self.start = limit;
        if flags.is_none() {
          return;
        }
        self.start = finish;
        self.maybe_output = None;
        self.results.push(RawResponse {
          num: u64::from_str(number.expect("if flags is present, so much number be")).unwrap(),
          output: block.clone(),
        })
      }
    }
  }

}
