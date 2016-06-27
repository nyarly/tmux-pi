#[macro_use]
extern crate lazy_static;
extern crate regex;

use std::sync::mpsc::SendError;
use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};
use std::sync::mpsc::{Receiver, Sender, channel};

pub struct TmuxControl {
  tmux: u32,
  reader: JoinHandle<()>,
  writer: JoinHandle<()>,
  commands: Sender<Pair>,
}

impl TmuxControl {
  pub fn start() -> TmuxControl {
    let child = Command::new("tmux")
                .arg("-C")
                .env_remove("TMUX")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
    let (responses, send) = channel();
    let (tell, commands) = channel();
    let pid = child.id();
    let stdin = Box::new(child.stdin.expect("didn't open a stdin pipe?!"));
    let stdout = Box::new(child.stdout.expect("didn't open a stdout pipe?!"));
    let writer = thread::spawn(move || command_stream::write(commands, responses, stdin));
    let reader = thread::spawn(move || command_stream::read(stdout, send));

    TmuxControl {
      tmux: pid, //how do we kill the child tmux process?
      reader: reader,
      writer: writer,
      commands: tell,
    }
  }

  /*
  fn stop(& mut self) -> io::Result<ExitStatus> {
    ()
    //self.tmux.kill().unwrap(); self.tmux.wait()
  }
  */

  fn transact(&self, cmd: Box<command::TmuxCommand>) ->
    Result<
      Receiver<Box<command::Response>>,
      SendError<Pair>
        > {
    let (tx, rx) = cmd.response_channel();
    try!(self.commands.send(Pair(cmd, tx)));
    Ok(rx)
  }
}

// XXX pub?
pub struct Pair(Box<command::TmuxCommand>, Sender<Box<command::Response>>);

mod command;
mod command_stream;
