use std::thread::{self, JoinHandle};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

#[macro_use]
extern crate lazy_static;
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

  fn transact(&self, cmd: Box<TmuxCommand>) -> Result(Receiver<Box<Response>>) {
    let (tx, rx) = cmd.response_channel();
    try!(self.commands.send(Pair(cmd, tx)));
    Ok(rx)
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
type StreamBuffer<'a> = &'a [u8];

use std::{str, u64};
use std::str::FromStr;
use std::io::{Read,BufRead,BufReader,Lines};
use std::sync::mpsc::TryRecvError;
use std::ops::{Deref,DerefMut};

struct CommandStreamReader {
  maybe_output: Option<String>,
  results: Vec<RawResponse>,
}

fn get_args(line: &str) -> (Option<&str>, Option<&str>, Option<&str>) {
  lazy_static!  {
    static ref arg_re: Regex = Regex::new(r"^(\s+(?<first>\S+))?(\s+(?<second>\S+))?(\s+(?<third>\S+))?\s*\n").unwrap();
  }
  let captures = arg_re.captures(&line).unwrap();
  (captures.at(1), captures.at(2), captures.at(3))
}

impl  CommandStreamReader  {
  fn new() -> CommandStreamReader {
    let max = 4096;
    CommandStreamReader{
      maybe_output: None,
      results : Vec::new(),

    }
  }

  fn read(&mut self, mut stdout: ChildStdout, send_channels: Receiver<SeqRs>) {
    let reader = BufReader::new(stdout);
    let mut maybe_output: Option<String> = None;
    for line_res in reader.lines() {
      let line = match line_res {
        Ok(l) => l,
        Err(e) => return
      };

      match maybe_output {
        None => self.capture_notification(line),
        Some(ref mut b) => self.capture_output_block(b, line),
      }

      while self.results.len() > 0 {
        match send_channels.try_recv() {
          Err(TryRecvError::Empty) => break,
          Err(_) => return,
          Ok(SeqRs(num, mut resp, tx)) => {
            let pos = self.results.iter()
                             .position(|ref r| r.num == num)
                             .expect("received response ahead of command");
            let res = self.results.swap_remove(pos);
            resp.consume(&(res.output));
            tx.send(resp).unwrap_or(())
          }
        }
      }

    }
  }

  fn capture_notification(&mut self, line: String) {
    lazy_static! {
      static ref stanza_re: Regex = Regex::new(r"^%(begin|exit|layout-change|output|session-changed|\
      session-renamed|sessions-changed|unlinked-window-add|window-add|window-close|\
      window-renamed)\s+(.*)").unwrap();
    }
    match stanza_re.captures(&line) {
      None => (),
      Some(captures) => {

        match captures.at(1).unwrap() {
          "begin" => {
            get_args(&line);
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

  fn capture_end(&mut self, block: String, line: &str) {
    let (timestamp, number, flags) = get_args(line);
    self.results.push(RawResponse {
      num: u64::from_str(number.expect("if flags is present, so must number be")).unwrap(),
      output: block,
    })
  }

  fn capture_output_block(&mut self, block: &mut String, line: String) {
    lazy_static! {
      static ref stanza_end_re: Regex = Regex::new(r"^%(end|error)").unwrap();
    }
    match stanza_end_re.captures(&line) {
      None => {
        block.push_str(&line);
      }
      Some(capture) => {
        self.capture_end(block.clone(), capture.at(1).unwrap());
      }
    }
  }

}
