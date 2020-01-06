macro_rules! matches {
    ($expression:expr, $( $pattern:pat )|+ $( if $guard: expr )?) => {
        match $expression {
            $( $pattern )|+ $( if $guard )? => true,
            _ => false
        }
    }
}

macro_rules! if_matches {
    ($expression:expr, $( $pattern:pat )|+ $( if $guard: expr )?, $bind: expr) => {
        match $expression {
            $( $pattern )|+ $( if $guard )? => Some($bind),
            _ => None
        }
    }
}

pub mod data;
pub mod magnet;
pub mod metainfo;
pub mod tracker;
