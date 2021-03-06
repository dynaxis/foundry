// Copyright 2020 Kodebox, Inc.
// This file is part of CodeChain.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

mod base;

use crate::sandbox::Sandbox;
use intertrait::CastFromSync;
use linkme::distributed_slice;
use once_cell::sync;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

type Result<T> = std::result::Result<T, Error>;

/// The list of functions for creating [`Linker`] implementations.
///
/// [`Linker`]: ./trait.Linker.html
#[distributed_slice]
pub static LINKERS: [fn() -> (&'static str, Arc<dyn Linker>)] = [..];

/// Returns a `Linker` with the given `id`.
pub fn linker(id: &str) -> Option<Arc<dyn Linker>> {
    static MAP: sync::Lazy<HashMap<&'static str, Arc<dyn Linker>>> =
        sync::Lazy::new(|| LINKERS.iter().map(|new| new()).collect());
    MAP.get(id).map(Arc::clone)
}

/// Picks the best `Linker` for the given pairs of `Linkable`s.
pub fn best_linker(a: &dyn Sandbox, b: &dyn Sandbox) -> Option<Arc<dyn Linker>> {
    // Assumes that a linker is always better than another regardless of Linkables involved.
    // So picks the first one in the list of linkers for a Linkable in common with the other
    // Linkable regardless of the linker's position in the list of supported linkers
    // for the latter.
    let linkers_for_a = a.supported_linkers();
    let linkers_for_b = b.supported_linkers();

    let linker_set: HashSet<_> = linkers_for_b.iter().cloned().collect();

    linkers_for_a.iter().find(|id| linker_set.contains(*id)).iter().flat_map(|id| linker(**id)).last()
}

/// A linker is responsible for linking to `Port`s if both of them support
/// the required common traits. Each linker must mark itself with `#[Linker]`
/// attribute.
pub trait Linker: Send + Sync {
    /// Links the two [`Port`]s together.
    ///
    /// [`Port`]: ./trait.Port.html
    fn link(&self, a: &mut dyn Port, b: &mut dyn Port) -> Result<()>;
}

/// An entity that can be linked with another `Linkable`.
pub trait Linkable: Send + Sync {
    /// Returns a list of [`Linker`] IDs in the order of preference.
    ///
    /// [`Linker`]: ./trait.Linker.html
    fn supported_linkers(&self) -> &'static [&'static str];

    /// Creates a new [`Port`] that can be linked with a [`Linker`].
    ///
    /// [`Port`]: ./trait.Port.html
    /// [`Linker`]: ./trait.Linker.html
    fn new_port(&mut self) -> Box<dyn Port>;

    /// Seals this `Linkable` in the sense that no more `Port` is created and linked.
    fn seal(&mut self);
}

/// A port represents an endpoint of a link between two [`Linkable`]s.
///
/// Before linking two ports, each may be set up with its [`export`] and [`import`] methods.
/// This trait is just the basic protocol and every `Port` it supposed to implement additional
/// traits for its supported link types.
///
/// [`Linkable`]: ./trait.Linkable.html
/// [`export`]: ./trait.Port.html#tymnethod.export
/// [`import`]: ./trait.Port.html#tymnethod.import
pub trait Port: CastFromSync {
    /// Sets to send a list of handles represented by the `ids` to the other end on link
    /// creation. The `ids` are indices into a list of service objects created when the module
    /// owning this port is loaded into a sandbox.CBOR map fed
    /// to the constructor function.
    fn export(&mut self, ids: &[usize]);

    /// Sets to which slots the handles received from the other end are to be assigned.
    ///
    /// This way, a module can't assign to an arbitrary slot in the other end.
    /// Only to the slots set by the host.
    fn import(&mut self, slots: &[&str]);
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The port from a linkable '{id}' is not supported by the linker")]
    UnsupportedPortType {
        id: &'static str,
    },
}
