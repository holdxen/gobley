/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::hash::{Hash, Hasher};
use std::sync::Arc;

uniffi::setup_scaffolding!();

// Test Record with trait methods
#[derive(uniffi::Record, Debug)]
pub struct UserProfile {
    pub name: String,
    pub age: u32,
    pub email: String,
}

// Custom implementation of PartialEq - only compare name and email
impl PartialEq for UserProfile {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.email == other.email
    }
}

impl Eq for UserProfile {}

impl Hash for UserProfile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.email.hash(state);
    }
}

impl Ord for UserProfile {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialOrd for UserProfile {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for UserProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (age: {})", self.name, self.age)
    }
}

#[uniffi::export]
fn create_user_profile(name: String, age: u32, email: String) -> UserProfile {
    UserProfile { name, age, email }
}

#[uniffi::export]
fn user_profile_to_string(profile: &UserProfile) -> String {
    profile.to_string()
}

// Test Enum with trait methods
#[derive(uniffi::Enum, Debug)]
pub enum ApiResponse {
    Success { data: String, code: u32 },
    Error { message: String, code: u32 },
    Loading,
}

// Custom PartialEq - only compare discriminant
impl PartialEq for ApiResponse {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Eq for ApiResponse {}

impl Hash for ApiResponse {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
    }
}

impl Ord for ApiResponse {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare by discriminant order: Loading < Error < Success
        let self_ord = match self {
            ApiResponse::Loading => 0,
            ApiResponse::Error { .. } => 1,
            ApiResponse::Success { .. } => 2,
        };
        let other_ord = match other {
            ApiResponse::Loading => 0,
            ApiResponse::Error { .. } => 1,
            ApiResponse::Success { .. } => 2,
        };
        self_ord.cmp(&other_ord)
    }
}

impl PartialOrd for ApiResponse {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for ApiResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiResponse::Success { data, code } => write!(f, "Success({code}): {data}"),
            ApiResponse::Error { message, code } => write!(f, "Error({code}): {message}"),
            ApiResponse::Loading => write!(f, "Loading..."),
        }
    }
}

#[uniffi::export]
fn create_success_response(data: String) -> ApiResponse {
    ApiResponse::Success { data, code: 200 }
}

#[uniffi::export]
fn create_error_response(message: String) -> ApiResponse {
    ApiResponse::Error {
        message,
        code: 500,
    }
}

#[uniffi::export]
fn get_loading_response() -> ApiResponse {
    ApiResponse::Loading
}

#[uniffi::export]
fn response_to_string(response: &ApiResponse) -> String {
    response.to_string()
}

// Test Object with trait methods
#[derive(Debug, uniffi::Object)]
pub struct Counter {
    value: std::sync::atomic::AtomicI32,
}

#[uniffi::export]
impl Counter {
    #[uniffi::constructor]
    fn new(initial: i32) -> Arc<Self> {
        Arc::new(Self {
            value: std::sync::atomic::AtomicI32::new(initial),
        })
    }

    fn increment(&self) -> i32 {
        self.value.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
    }

    fn get_value(&self) -> i32 {
        self.value.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// Test flat enum
#[derive(uniffi::Enum, Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum Color {
    Red,
    Green,
    Blue,
    Custom(u32),
}

#[uniffi::export]
fn get_color_name(color: Color) -> String {
    match color {
        Color::Red => "Red".to_string(),
        Color::Green => "Green".to_string(),
        Color::Blue => "Blue".to_string(),
        Color::Custom(code) => format!("Custom({code})"),
    }
}

#[uniffi::export]
fn is_primary_color(color: Color) -> bool {
    matches!(color, Color::Red | Color::Green | Color::Blue)
}

// Test nested types
#[derive(uniffi::Record, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(uniffi::Record, Debug, PartialEq)]
pub struct Rectangle {
    pub top_left: Point,
    pub bottom_right: Point,
}

#[uniffi::export]
fn create_rectangle(x1: f64, y1: f64, x2: f64, y2: f64) -> Rectangle {
    Rectangle {
        top_left: Point { x: x1, y: y1 },
        bottom_right: Point { x: x2, y: y2 },
    }
}

#[uniffi::export]
fn get_rectangle_area(rect: &Rectangle) -> f64 {
    let width = (rect.bottom_right.x - rect.top_left.x).abs();
    let height = (rect.bottom_right.y - rect.top_left.y).abs();
    width * height
}

// Test Optional and Vec types
#[uniffi::export]
fn find_first_even(numbers: Vec<i32>) -> Option<i32> {
    numbers.into_iter().find(|n| n % 2 == 0)
}

#[uniffi::export]
fn merge_strings(strings: Vec<String>, separator: &str) -> String {
    strings.join(separator)
}

// Test error handling
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ValidationError {
    #[error("Field '{field}' is required")]
    RequiredField { field: String },
    #[error("Value must be between {min} and {max}")]
    OutOfRange { min: i32, max: i32 },
    #[error("Invalid format")]
    InvalidFormat,
}

#[uniffi::export]
fn validate_age(age: i32) -> Result<u32, ValidationError> {
    if age < 0 {
        Err(ValidationError::OutOfRange { min: 0, max: 150 })
    } else if age > 150 {
        Err(ValidationError::OutOfRange { min: 0, max: 150 })
    } else {
        Ok(age as u32)
    }
}

#[uniffi::export]
fn validate_name(name: &str) -> Result<String, ValidationError> {
    if name.is_empty() {
        Err(ValidationError::RequiredField {
            field: "name".to_string(),
        })
    } else if name.len() > 100 {
        Err(ValidationError::InvalidFormat)
    } else {
        Ok(name.to_string())
    }
}
