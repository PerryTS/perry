wit_bindgen::generate!({ path: "..", world: "plugin" });
use exports::perry::host_resources::runner::{Guest, Pair};
use perry::host_resources::{
    aliases,
    tokens::{Other, Token},
};
use std::cell::RefCell;

thread_local! { static HELD: RefCell<Option<Token>> = const { RefCell::new(None) }; }
struct Runner;
impl Guest for Runner {
    fn make(value: u32) -> Token {
        Token::new(value)
    }
    fn echo(value: Token) -> Token {
        value
    }
    fn read(value: &Token) -> u32 {
        value.read()
    }
    fn read_alias(value: &Token) -> u32 {
        aliases::read_alias(value)
    }
    fn consume(value: Token) {
        drop(value);
    }
    fn consume_other(value: Other) {
        drop(value);
    }
    fn consume_imported(value: Token) {
        perry::host_resources::tokens::consume(value);
    }
    fn return_to_host(value: Token) {
        perry::host_resources::tokens::hold(value);
    }
    fn consume_pair(value: Pair) {
        drop(value);
    }
    fn checked(value: Token, _: u8) {
        drop(value);
    }
    fn mixed(owner: Token, _: &Token) {
        drop(owner);
    }
    fn keep(value: Token) {
        HELD.with(|held| *held.borrow_mut() = Some(value));
    }
    fn take() -> Token {
        HELD.with(|held| held.borrow_mut().take().expect("held resource"))
    }
    fn fail(_: Token) {
        core::arch::wasm32::unreachable();
    }
}
export!(Runner);
