/*
 * Test fixture for proc-macro based record/enum methods.
 * This crate uses #[uniffi::export] impl to define methods on records and enums,
 * which exercises the library mode codegen path (cdylib → metadata → bindings).
 */

use std::sync::Arc;

uniffi::setup_scaffolding!();

// ─── Record with methods via #[uniffi::export] impl ───

#[derive(uniffi::Record, Debug)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[uniffi::export]
impl Point {
    fn distance_to(&self, other: &Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    fn to_string_debug(&self) -> String {
        format!("({}, {})", self.x, self.y)
    }
}

// ─── Record with Display trait (uniffi_trait_methods) ───

#[derive(uniffi::Record, Debug)]
pub struct UserProfile {
    pub name: String,
    pub age: u32,
}

impl std::fmt::Display for UserProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (age: {})", self.name, self.age)
    }
}

// ─── Enum with methods via #[uniffi::export] impl ───

#[derive(uniffi::Enum, Debug)]
pub enum Direction {
    North,
    South,
    East,
    West,
}

#[uniffi::export]
impl Direction {
    fn name(&self) -> String {
        format!("{:?}", self)
    }

    fn opposite(&self) -> Direction {
        match self {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
        }
    }
}

// ─── Enum with Display trait ───

#[derive(uniffi::Enum, Debug)]
pub enum Color {
    Red,
    Green,
    Blue,
}

impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Color::Red => write!(f, "Red"),
            Color::Green => write!(f, "Green"),
            Color::Blue => write!(f, "Blue"),
        }
    }
}

// ─── Sealed enum (with data) and methods ───

#[derive(uniffi::Enum, Debug)]
pub enum ApiResponse {
    Success { data: String, code: u32 },
    Error { message: String },
}

#[uniffi::export]
impl ApiResponse {
    fn is_success(&self) -> bool {
        matches!(self, ApiResponse::Success { .. })
    }

    fn status_code(&self) -> u32 {
        match self {
            ApiResponse::Success { code, .. } => *code,
            ApiResponse::Error { .. } => 0,
        }
    }
}

// ─── Object for testing alongside records/enums ───

#[derive(uniffi::Object)]
pub struct Calculator {
    value: std::sync::Mutex<f64>,
}

#[uniffi::export]
impl Calculator {
    #[uniffi::constructor]
    fn new(initial: f64) -> Arc<Self> {
        Arc::new(Self {
            value: std::sync::Mutex::new(initial),
        })
    }

    fn add(&self, amount: f64) -> f64 {
        let mut v = self.value.lock().unwrap();
        *v += amount;
        *v
    }

    fn get_value(&self) -> f64 {
        *self.value.lock().unwrap()
    }
}

// ─── Top-level functions ───

#[uniffi::export]
fn create_point(x: f64, y: f64) -> Point {
    Point { x, y }
}

#[uniffi::export]
fn point_distance(a: &Point, b: &Point) -> f64 {
    a.distance_to(b)
}

// ─── Rename tests: #[uniffi(name = "...")] ───

// Record with renamed type
#[derive(uniffi::Record, Debug)]
#[uniffi(name = "RenamedPoint")]
pub struct PrivatePoint {
    pub x: f64,
    pub y: f64,
}

// Record with renamed method
#[derive(uniffi::Record, Debug)]
pub struct Vector2D {
    pub dx: f64,
    pub dy: f64,
}

#[uniffi::export]
impl Vector2D {
    fn length(&self) -> f64 {
        (self.dx * self.dx + self.dy * self.dy).sqrt()
    }

    fn scale(&self, factor: f64) -> Vector2D {
        Vector2D {
            dx: self.dx * factor,
            dy: self.dy * factor,
        }
    }
}

// Enum with renamed type
#[derive(uniffi::Enum, Debug)]
#[uniffi(name = "RenamedStatus")]
pub enum InternalStatus {
    Active,
    Inactive,
}

// Enum with renamed variant (via field-level rename is not supported for enums,
// but the enum type itself can be renamed)

// Renamed top-level function
#[uniffi::export(name = "calculate_sum")]
fn internal_sum(a: i32, b: i32) -> i32 {
    a + b
}

// ─── Object with renamed type and methods ───

#[derive(uniffi::Object)]
pub struct InternalCalc {
    value: std::sync::Mutex<f64>,
}

#[uniffi::export]
impl InternalCalc {
    #[uniffi::constructor]
    fn new(initial: f64) -> Arc<Self> {
        Arc::new(Self {
            value: std::sync::Mutex::new(initial),
        })
    }

    #[uniffi::method(name = "compute")]
    fn add(&self, amount: f64) -> f64 {
        let mut v = self.value.lock().unwrap();
        *v += amount;
        *v
    }

    #[uniffi::method(name = "result")]
    fn get_value(&self) -> f64 {
        *self.value.lock().unwrap()
    }
}

// Record with renamed field
#[derive(uniffi::Record, Debug)]
pub struct Config {
    #[uniffi(name = "configName")]
    pub internal_name: String,
    #[uniffi(name = "configValue")]
    pub internal_value: i32,
}
