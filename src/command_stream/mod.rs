use regex::Regex;

//use std::{str, u64};
use std::str::{self,FromStr};
use std::io::{Read,Write,BufRead,BufReader};
use std::sync::mpsc::{TryRecvError,Receiver, Sender,SendError};

type StreamBuffer<'a> = &'a [u8];

use super::command::Response;

#[derive(Debug)]
pub struct SeqRs(u64, Box<Response>, Sender<Box<Response>>);

lazy_static!  {
  static ref ARG_RE: Regex = Regex::new(r"^(\s+(?<first>\S+))?(\s+(?<second>\S+))?(\s+(?<third>\S+))?\s*\n").unwrap();
  static ref STANZA_RE: Regex = Regex::new(r"^%(begin|exit|layout-change|output|session-changed|\
    session-renamed|sessions-changed|unlinked-window-add|window-add|window-close|\
    window-renamed)\s+(.*)").unwrap();
  static ref STANZA_END_RE: Regex = Regex::new(r"^%(end|error)").unwrap();
}

struct RawResponse {
  num: u64,
  output: String,
}

struct Reader {
  maybe_output: Option<String>,
  results: Vec<RawResponse>,
}

mod test;

pub fn write<T: Write>(commands: Receiver<super::Pair>, results: Sender<SeqRs>, mut stdin:T) -> T {
  println!("write");
  let message: String;
  let mut seq = 0;
  for pair in commands.iter() {
    let super::Pair(command, tx) = pair;
    seq += 1;
    println!("Command: {:?}#{:?}", str::from_utf8(command.wire_format()).unwrap(), seq);
    match results.send(SeqRs(seq, command.build_response(), tx)) {
      Err(SendError(d)) => {
      println!("Err sending: {:?}", d);
      return stdin
      },
      _ => ()
    }
    let wr = stdin.write(command.wire_format());
    if wr.is_err() {
      println!("Err writing: {:?}", wr);
      return stdin
    }
  }; stdin
}

pub fn read(stdout: Box<Read>, send_channels: Receiver<SeqRs>) {
  let mut csr = Reader::new();
  csr.read(stdout, send_channels)
}

fn get_args(line: &str) -> (Option<&str>, Option<&str>, Option<&str>) {
  let captures = ARG_RE.captures(&line).unwrap();
  (captures.at(1), captures.at(2), captures.at(3))
}

impl  Reader  {
  fn new() -> Reader {
    Reader{
      maybe_output: None,
      results : Vec::new(),

    }
  }

  fn read(&mut self, mut stdout: Box<Read>, send_channels: Receiver<SeqRs>) {
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
    match STANZA_RE.captures(&line) {
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
    match STANZA_END_RE.captures(&line) {
      None => {
        block.push_str(&line);
      }
      Some(capture) => {
        self.capture_end(block.clone(), capture.at(1).unwrap());
      }
    }
  }

}
