//! Reed-Solomon Error Correction Tests
//!
//! Organized test suite for Reed-Solomon implementation including:
//! - Galois field arithmetic (galois.rs)
//! - Reconstruction engine (reconstruction.rs)  
//! - Property-based tests (property.rs)

mod reed_solomon {
    pub mod galois;
    pub mod property;
    pub mod reconstruction;
}
