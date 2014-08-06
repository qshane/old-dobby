#[phase(plugin)]
extern crate peg_syntax_ext;

use std::fmt::{Show, Formatter, FormatError};
use std::slice::Items;

peg! response(r#"
use command::{Atom,Command,KeyValue,Flag,Pipe,Raw,Arguments,combine,combine_boxed};

#[export]
atom -> Atom
	= c:ident " " v:arguments { Command(c, v) }
	/ p:pipe_sequence { Pipe(p) }
	/ r:raw { r }

pipe_sequence -> Vec<Box<Atom>>
	= l:arguments "|" r:pipe_sequence { combine_boxed(l, r) }
	/ a:arguments { vec!( a ) }

arguments -> Box<Atom>
	= a:argument_sequence { box Arguments(a) }

argument_sequence -> Vec<Atom>
	= head:argument " " tail:argument_sequence { combine(head, tail) }
	/ a:argument { vec!(a) }

argument -> Atom
	= keyvalue
	/ flag
	/ raw

flag -> Atom
	= "-" v:value { Flag(v) }

raw -> Atom
	= v:ident { Raw(v) }

keyvalue -> Atom
	= name:ident "=" val:value { KeyValue(name, val) }

ident -> String
	= l:ival r:ident { l + r }
	/ l:ival { l }

ival -> String
	= !isyntax . { match_str.to_string() }

value -> String
	= l:cval r:value { l + r }
	/ l:cval { l }

cval -> String
	= !vsyntax . { match_str.to_string() }

vsyntax -> String
	= "|" { "|".to_string() }
	/ " " { " ".to_string() }

isyntax -> String
	= "=" { "=".to_string() }
	/ "|" { "|".to_string() }
	/ " " { " ".to_string() }

"#)

#[deriving(Clone)]
pub enum Atom {
	Command(String, Box<Atom>),
	KeyValue(String, String),
	Flag(String),
	Pipe(Vec<Box<Atom>>),
	Raw(String),
	Arguments(Vec<Atom>)
}

pub fn combine(init: Atom, second: Vec<Atom>) -> Vec<Atom> {
	let mut tmp = vec!(init);
	tmp.push_all_move(second);
	tmp
}

pub fn combine_boxed(init: Box<Atom>, second: Vec<Box<Atom>>) -> Vec<Box<Atom>> {
	let mut tmp = vec!(init);
	tmp.push_all_move(second);
	tmp
}

pub fn parse(msg: &String) -> Result<Atom, String> {
	response::atom(msg.as_slice())
}

pub fn escape(start: &String) -> String {
	start
		.replace("\\", "\\\\")
		.replace("/", "\\/")
		.replace(" ", "\\s")
		.replace("|", "\\p")
		.replace("\n", "\\n")
		.replace("\r", "\\r")
		.replace("\t", "\\t")
}

pub fn unescape(start: &String) -> String {
	start
		.replace("\\\\", "\\")
		.replace("\\/", "/")
		.replace("\\s", " ")
		.replace("\\p", "|")
		.replace("\\n", "\n")
		.replace("\\r", "\r")
		.replace("\\t", "\t")
}

impl Atom {
	pub fn iter_pipe(&self) -> Items<Box<Atom>> {
		match *self {
			Pipe(ref vec) => {
				return vec.iter();
			},
			_ => {
				fail!("parse error! not a pipe: {}", self);
			}
		}
	}

	pub fn iter_args(&self) -> Items<Atom> {
		match *self {
			Arguments(ref vec) => {
				return vec.iter();
			},
			_ => {
				fail!("parse error! not arguments: {}", self);
			}
		}
	}

	pub fn iter_cmd(&self) -> Items<Atom> {
		match *self {
			Command(_, ref args) => {
				return args.iter_args()
			},
			_ => {
				fail!("parse error! not a command: {}", self);
			}
		}
	}
}

impl Show for Atom {
	fn fmt(&self, format: &mut Formatter) -> Result<(), FormatError> {
		match self {
			&Command(ref name, ref arguments) => {
				format_args!(|args| {
					format.write_fmt(args);
				}, "{} {}", name, arguments);
			},
			&KeyValue(ref key, ref value) => {
				format_args!(|args| {
					format.write_fmt(args);
				}, "{}={}", key, escape(value))
			},
			&Pipe(ref atom) => {
				let mut first = true;

				for atom in atom.iter() {
					if first {
						format_args!(|args| {
							format.write_fmt(args);
						}, "{}", atom);
					} else {
						format_args!(|args| {
							format.write_fmt(args);
						}, "|{}", atom);
					}

					first = false;
				}
			},
			&Flag(ref flag) => {
				format_args!(|args| {
					format.write_fmt(args);
				}, "-{}", flag);
			},
			&Raw(ref flag) => {
				format_args!(|args| {
					format.write_fmt(args);
				}, "{}", flag);
			},
			&Arguments(ref vec) => {
				let mut first = true;

				for atom in vec.iter() {
					if first {
						format_args!(|args| {
							format.write_fmt(args);
						}, "{}", atom);
					} else {
						format_args!(|args| {
							format.write_fmt(args);
						}, " {}", atom);
					}
					first = false;
				}
			}
		}
		return Ok(());
	}
}
