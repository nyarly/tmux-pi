#![cfg(test)]

use command::TmuxCommand;
use std::str;
use std::string::String;
use std::sync::mpsc::{channel, Sender};
use super::*;

#[test]
fn writes_commands_to_stream() {
  use std::io::{Cursor, Write};
  let (ct, cr) = channel();
  let (rt, rr) = channel();
  let mut stdin = Box::new(Cursor::new(vec![]));

  let cmd_str = send_test_cmd(ct);

  stdin = write(cr, rt, stdin);

  assert_eq!(String::from_utf8(stdin.into_inner()).unwrap(), cmd_str)
}

#[test]
fn reads_responses_from_stream() {
  read()

}

fn send_test_cmd(ct: Sender<::Pair>) -> String {
  let (res_t, _) = channel();
  let cmd = ::command::info::new();
  let wf = String::from(str::from_utf8(cmd.wire_format()).unwrap());

  assert!(ct.send(::Pair(cmd, res_t)).is_ok());
  wf
}
