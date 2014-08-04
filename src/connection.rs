use command;

use std::io::TcpStream;
use std::sync::{Arc,Mutex};
use std::io::{IoResult,IoError};

fn dispatch(message: Result<command::Atom, uint>, callback: Sender<(Result<command::Atom, uint>, Sender<Result<(), String>>)>) -> Result<(), String> {
	let (tx, rx) = channel();
	callback.send((message, tx));
	return rx.recv();
}

pub struct TS3Connection <'a> {
	socket: Arc<Mutex<TcpStream>>
}

impl<'a> TS3Connection<'a> {
	pub fn new<'a>(host: &str) -> IoResult<Arc<Box<TS3Connection<'a>>>> {
		return Ok(Arc::new(
			box TS3Connection {
				socket: Arc::new(Mutex::new(match TcpStream::connect(host, 10011) {
					Ok(stream) => stream,
					Err(e) => {return Err(e)}
				}))
			}
		));
	}

	fn get_line(&self) -> IoResult<String> {
		let mut socket = self.get_socket();
		let mut result: Vec<u8> = Vec::new();

		loop {
			let r = socket.read_byte();

			match r {
				Ok(b) => {
					if b == 13 {
						return Ok(String::from_utf8(result).unwrap());
					} else if b == 10 {
						// ignore?
					} else {
						result.push(b);
					}
				},
				Err(e) => {
					return Err(e);
				}
			}
		}
	}

	fn get_socket(&self) -> TcpStream {
		let mut sock = self.socket.lock();
		return (*sock).clone();
	}

	pub fn write(&self, data: &str) -> Result<(), IoError> {
		//println!("SENDING: {}", data);
		let mut socket = self.get_socket();

		match socket.write(data.as_bytes()) {
			Err(e) => {return Err(e);},
			_ => {}
		}
		match socket.write("\n".as_bytes()) {
			Err(e) => {return Err(e);},
			_ => {}
		}

		return Ok(());
	}

	pub fn read(&self, notify: Sender<command::Atom>, message: Sender<(Result<command::Atom, uint>, Sender<Result<(), String>>)>) -> Result<(), String> {
		let data_buffer: &mut Option<command::Atom> = &mut None;
		let mut received = 0u;
		loop {
			match self.get_line() {
				Ok(line) => {
					received += 1;
					//println!("GOT LINE: {}", line);
					if received > 2 {
						match command::parse(&line) {
							Ok(parsed) => {
								match parsed {
									command::Command(ref name, ref arguments) => {
										if name.as_slice() == "error" {
											match **arguments {
												command::Arguments(ref vec) => {
													for atom in vec.iter() {
														match *atom {
															command::KeyValue(ref k, ref v) => {
																if k.as_slice() == "id" {
																	let code = from_str::<uint>(v.as_slice()).unwrap();
																	if code != 0 {
																		match dispatch(Err(code), message.clone()) {
																			Ok(_) => {},
																			Err(e) => {
																				return Err(e);
																			}
																		}

																		*data_buffer = None;
																	} else {
																		if data_buffer.is_none() {
																			match dispatch(Ok(parsed.clone()), message.clone()) {
																				Ok(_) => {},
																				Err(e) => {
																					return Err(e);
																				}
																			}
																		} else {
																			match dispatch(Ok(data_buffer.clone().unwrap()), message.clone()) {
																				Ok(_) => {},
																				Err(e) => {
																					return Err(e);
																				}
																			}
																			*data_buffer = None;
																		}
																	}
																}
															},
															_ => {
																*data_buffer = Some(parsed.clone());
															}
														}
													}
												},
												_ => {
													*data_buffer = Some(parsed.clone());
												}
											}
										} else if name.as_slice() == "notifytextmessage" {
											notify.send(parsed.clone());
											*data_buffer = None;
										}
									},
									_ => {
										*data_buffer = Some(parsed.clone());
									}
								}
							},
							Err(_) => {
								return Err(format!("Couldn't parse: {}", line))
							}
						}
					}
				},
				Err(e) => {
					self.close();
					return Err(format!("IO error: {}", e));
				}
			}
		}
	}

	pub fn close(&self) {
		let mut socket = self.get_socket();

		let _ = socket.close_read();
		let _ = socket.close_write();
	}
}