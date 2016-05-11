use std::thread::{self, JoinHandle};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

extern crate regex;

struct TmuxControl {
  tmux: Child,
  reader: JoinHandle<()>,
  writer: JoinHandle<()>,
  commands: Sender<Pair>,
}

struct Pair(Box<TmuxCommand<R=Response>>, Sender<Box<Response>>);
struct SeqRs(u64, Box<Response>, Sender<Box<Response>>);

impl TmuxControl {
  fn start() -> TmuxControl {
    let cmd = Command::new("tmux")
                .arg("-C")
                .env_remove("TMUX")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
    let child = cmd.spawn().unwrap();
    let (responses, send) = channel();
    let (tell, commands) = channel();
    let writer = thread::spawn(move || write_command_stream(commands, responses, child.stdin.expect("didn't open a stdin pipe?!")));
    let reader = thread::spawn(move || read_command_stream(child.stdout.expect("didn't open a stdout pipe?!"), send));

    TmuxControl {
      tmux: child,
      reader: reader,
      writer: writer,
      commands: tell,
    }
  }

  fn transact(&self, cmd: Box<TmuxCommand<R=Response>>) -> Receiver<Box<Response>> {
    let (tx, rx) = cmd.response_channel();
    self.commands.send(Pair(cmd, tx));
    rx
  }
}

trait TmuxCommand : Send{
  type R: Response;

  fn response_channel(&mut self) -> (Sender<Box<Self::R>>, Receiver<Box<Self::R>>) {
    channel()
  }
  fn build_response(&self) -> Box<Self::R>;
  fn wire_format(&self) -> &'static [u8];
}

trait Response : Send {
  fn consume(&mut self, text: &[u8]);
}

struct InfoCmd;

struct InfoResponse {
  content: String
}

impl TmuxCommand for InfoCmd {
  type R = InfoResponse;

  fn build_response(&self) -> Box<InfoResponse> {
    Box::new(InfoResponse::new())
  }
  fn wire_format(&self) -> &'static [u8] {
    b"info"
  }
}

impl Response for InfoResponse {
  fn consume(&mut self, text: &[u8]) {
    self.content = String::from_utf8_lossy(text).into_owned()
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

fn write_command_stream(commands: Receiver<Pair>, results: Sender<SeqRs>, stdin: ChildStdin) {
  let message: String;
  let seq = 0;
  loop {
    let Pair(command, tx) = commands.recv().unwrap();
    seq += 1;
    results.send(SeqRs(seq, command.build_response(), tx));
    let res = stdin.write(command.wire_format());
    res.unwrap();
  }
}

fn read_command_stream(stdout: ChildStdout, send_channels: Receiver<SeqRs>) {
  let mut buffer = [0; 4096];
  let (start, end, max) = (0, 0, 4096);
  let half = max / 2;
  let response_mode = false;
  let maybe_output: Option<String> = None;
  let mut results = Vec::new();

  let stanza_re = Regex::new("(?m)^%(begin|exit|layout-change|output|session-changed|\
    session-renamed|sessions-changed|unlinked-window-add|window-add|window-close|\
    window-renamed)")
                    .unwrap();
  let stanza_end_re = Regex::new("(?m)^%(end|error)").unwrap();
  let max_end_length = 6;

  let arg_re = Regex::new(r"^(\s+(?<first>\S+))?(\s+(?<second>\S+))?(\s+(?<third>\S+))?\s*\n");

  let advance_buffer = |captures: &Captures| {
    let (b, e) = captures.pos(0).unwrap();
    start += e;
  };

  let get_args = || -> (Option(&str), Option(&str), Option(&str)) {
    let captures = arg_re.captures(buffer[start..end]).unwrap();
    advance_buffer(captures);
    (captures.at(1), captures.at(2), captures.at(3));
  };

  let capture_notification = || {
    match stanza_re.captures(buffer[start..end]) {
      None => (),
      Some(captures) => {
        advance_buffer(&captures);

        match captures.at(1).unwrap() {
          "begin" => {
            get_args();
            maybe_output = Some(String::from(""))
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
        }
      }
    }
  };

  let capture_output_block = |block| {
    let limit: u16;
    match stanza_end_re.captures(buffer[start..end]) {
      None => {
        limit = end - (max_end_length);
        block.extend(buffer[start..limit]);
        start = limit;
      }
      Some(capture) => {
        let (limit, finish) = capture.pos(0);
        let (timestamp, number, flags) = get_args();
        block.extend(buffer[start..limit]);
        start = limit;
        if flags.is_none() {
          return;
        }
        start = finish;
        maybe_output = None;
        results.push(RawResponse {
          num: u64::from_str(number).unwrap(),
          output: block,
        })
      }
    }
  };

  loop {
    match stdout.read(&mut buffer[end..(max - 1)]) {
      Ok(num) => end += num,
      Err(err) => panic!(err),
    }

    match maybe_output {
      None => capture_notification(),
      Some(block) => capture_output_block(&block),
    }

    while results.len() > 0 {
      match send_channels.try_recv() {
        Err(Empty) => break,
        Err(_) => return,
        Ok(SeqRs(num, resp, tx)) => {
          let pos = results.iter()
                           .position(|&r| r.num == num)
                           .expect("received response ahead of command");
          let res = results.swap_remove(pos);
          resp.consume(res.output);
          tx.send(resp)
        }
      }
    }

    if start > half {
      buffer[..(end - start)].clone_from_slice(buffer[start..end]);
      (start, end) = (0, end - start)
    }
  }
}
