/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::collections::HashSet;

#[uniffi::export]
fn string_set_identity(set: HashSet<String>) -> HashSet<String> {
    set
}

#[uniffi::export]
fn make_string_set(values: Vec<String>) -> HashSet<String> {
    values.into_iter().collect()
}

#[uniffi::export]
fn box_u32_identity(value: Box<u32>) -> Box<u32> {
    value
}

uniffi::include_scaffolding!("set-and-box");
