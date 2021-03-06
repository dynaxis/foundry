// Copyright 2019-2020 Kodebox, Inc.
// This file is part of CodeChain.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

#![feature(test)]

extern crate codechain_crypto as ccrypto;
extern crate codechain_key as ckey;
extern crate test;

use ckey::{sign, verify, Ed25519KeyPair, Generator, KeyPairTrait, Message, Random};
use test::Bencher;

#[bench]
fn tendermint_max_step_time(b: &mut Bencher) {
    // Based on prevote/precommit state.
    let num_validators = 30;

    let key_pair_self: Ed25519KeyPair = Random.generate().unwrap();
    let message_self = Message::random();
    let mut key_pairs = vec![];
    let mut messages = vec![];
    let mut signatures = vec![];
    let mut i = 0;

    while i < num_validators - 1 {
        let key_pair: Ed25519KeyPair = Random.generate().unwrap();
        let message = Message::random();
        let signature = sign(message.as_ref(), key_pair.private());

        key_pairs.push(key_pair);
        messages.push(message);
        signatures.push(signature);

        i += 1;
    }
    b.iter(|| {
        sign(message_self.as_ref(), key_pair_self.private());

        let mut i = 0;
        while i < num_validators - 1 {
            assert!(verify(&signatures[i], messages[i].as_ref(), key_pairs[i].public()));
            i += 1;
        }
    });
}
