//! Tables and data models.
//!
//! # Overview
//!
//! This module defines the tables in reth, as well as some table-related abstractions:
//!
//! - [`codecs`] integrates different codecs into [`Encode`] and [`Decode`]
//! - [`models`](crate::models) defines the values written to tables
//!
//! # Database Tour
//!
//! TODO(onbjerg): Find appropriate format for this...

use alloy_primitives::{BlockNumber, Bytes};
use reth_db::{table::TableInfo, tables, TableSet, TableType, TableViewer};
use std::fmt;

tables! {
    /// Read precompile calls for each block.
    table BlockReadPrecompileCalls {
        type Key = BlockNumber;
        type Value = Bytes;
    }
}
