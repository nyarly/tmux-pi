#![cfg(test)]

use super::*;
use std::sync::mpsc::{channel,Sender};
use std::panic::catch_unwind;
use command::TmuxCommand;

#[test]
fn writes_commands_to_stream() {
  let (ct, cr) = channel();
  let (rt, rr) = channel();
  let stdin: Vec<u8> = Vec::new();

  let cmd = send_test_cmd(ct);
  write(cr, rt, Box::new(stdin));

  assert_eq!(cmd.wire_format(), &stdin[..])
}

fn send_test_cmd(ct: Sender<::Pair>) -> Box<::command::info::Cmd> {
  let (resT, resR) = channel();
  let cmd = ::command::info::new();

  ct.send(::Pair(cmd, resT));
  cmd
}
