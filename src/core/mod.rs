mod executor;
pub mod index;
mod index_meta;
mod inverted_index_reader;
pub mod searcher;
mod segment;
mod segment_component;
mod segment_id;
mod segment_reader;
mod single_segment_index_writer;

use std::path::Path;

use once_cell::sync::Lazy;

pub use self::executor::Executor;
pub use self::index::{Index, IndexBuilder};
pub use self::index_meta::{
    IndexMeta, IndexSettings, IndexSortByField, Order, SegmentMeta, SegmentMetaInventory,
};
pub use self::inverted_index_reader::InvertedIndexReader;
pub use self::searcher::{Searcher, SearcherGeneration};
pub use self::segment::Segment;
pub use self::segment_component::SegmentComponent;
pub use self::segment_id::SegmentId;
pub use self::segment_reader::SegmentReader;
pub use self::single_segment_index_writer::SingleSegmentIndexWriter;

/// The meta file contains all the information about the list of segments and the schema
/// of the index.
pub static META_FILEPATH: Lazy<&'static Path> = Lazy::new(|| Path::new("meta.json"));

/// The managed file contains a list of files that were created by the tantivy
/// and will therefore be garbage collected when they are deemed useless by tantivy.
///
/// Removing this file is safe, but will prevent the garbage collection of all of the file that
/// are currently in the directory
pub static MANAGED_FILEPATH: Lazy<&'static Path> = Lazy::new(|| Path::new(".managed.json"));
