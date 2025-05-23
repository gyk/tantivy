#![doc(html_logo_url = "http://fulmicoton.com/tantivy-logo/tantivy-logo.png")]
#![cfg_attr(all(feature = "unstable", test), feature(test))]
#![doc(test(attr(allow(unused_variables), deny(warnings))))]
#![warn(missing_docs)]
#![allow(
    clippy::len_without_is_empty,
    clippy::derive_partial_eq_without_eq,
    clippy::module_inception,
    clippy::needless_range_loop,
    clippy::bool_assert_comparison
)]

//! # `tantivy`
//!
//! Tantivy is a search engine library.
//! Think `Lucene`, but in Rust.
//!
//! ```rust
//! # use std::path::Path;
//! # use tempfile::TempDir;
//! # use tantivy::collector::TopDocs;
//! # use tantivy::query::QueryParser;
//! # use tantivy::schema::*;
//! # use tantivy::{doc, DocAddress, Index, Score};
//! #
//! # fn main() {
//! #     // Let's create a temporary directory for the
//! #     // sake of this example
//! #     if let Ok(dir) = TempDir::new() {
//! #         run_example(dir.path()).unwrap();
//! #         dir.close().unwrap();
//! #     }
//! # }
//! #
//! # fn run_example(index_path: &Path) -> tantivy::Result<()> {
//! // First we need to define a schema ...
//!
//! // `TEXT` means the field should be tokenized and indexed,
//! // along with its term frequency and term positions.
//! //
//! // `STORED` means that the field will also be saved
//! // in a compressed, row-oriented key-value store.
//! // This store is useful to reconstruct the
//! // documents that were selected during the search phase.
//! let mut schema_builder = Schema::builder();
//! let title = schema_builder.add_text_field("title", TEXT | STORED);
//! let body = schema_builder.add_text_field("body", TEXT);
//! let schema = schema_builder.build();
//!
//! // Indexing documents
//!
//! let index = Index::create_in_dir(index_path, schema.clone())?;
//!
//! // Here we use a buffer of 100MB that will be split
//! // between indexing threads.
//! let mut index_writer = index.writer(100_000_000)?;
//!
//! // Let's index one documents!
//! index_writer.add_document(doc!(
//!     title => "The Old Man and the Sea",
//!     body => "He was an old man who fished alone in a skiff in \
//!             the Gulf Stream and he had gone eighty-four days \
//!             now without taking a fish."
//! ))?;
//!
//! // We need to call .commit() explicitly to force the
//! // index_writer to finish processing the documents in the queue,
//! // flush the current index to the disk, and advertise
//! // the existence of new documents.
//! index_writer.commit()?;
//!
//! // # Searching
//!
//! let reader = index.reader()?;
//!
//! let searcher = reader.searcher();
//!
//! let query_parser = QueryParser::for_index(&index, vec![title, body]);
//!
//! // QueryParser may fail if the query is not in the right
//! // format. For user facing applications, this can be a problem.
//! // A ticket has been opened regarding this problem.
//! let query = query_parser.parse_query("sea whale")?;
//!
//! // Perform search.
//! // `topdocs` contains the 10 most relevant doc ids, sorted by decreasing scores...
//! let top_docs: Vec<(Score, DocAddress)> =
//!     searcher.search(&query, &TopDocs::with_limit(10))?;
//!
//! for (_score, doc_address) in top_docs {
//!     // Retrieve the actual content of documents given its `doc_address`.
//!     let retrieved_doc = searcher.doc(doc_address)?;
//!     println!("{}", schema.to_json(&retrieved_doc));
//! }
//!
//! # Ok(())
//! # }
//! ```
//!
//!
//!
//! A good place for you to get started is to check out
//! the example code (
//! [literate programming](https://tantivy-search.github.io/examples/basic_search.html) /
//! [source code](https://github.com/quickwit-oss/tantivy/blob/main/examples/basic_search.rs))

#[cfg_attr(test, macro_use)]
extern crate serde_json;
#[macro_use]
extern crate log;

#[macro_use]
extern crate thiserror;

#[cfg(all(test, feature = "unstable"))]
extern crate test;

#[cfg(feature = "mmap")]
#[cfg(test)]
mod functional_test;

#[macro_use]
mod macros;
mod future_result;

/// Re-export of the `time` crate
///
/// Tantivy uses [`time`](https://crates.io/crates/time) for dates.
pub use time;

use crate::time::format_description::well_known::Rfc3339;
use crate::time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

/// A date/time value with microsecond precision.
///
/// This timestamp does not carry any explicit time zone information.
/// Users are responsible for applying the provided conversion
/// functions consistently. Internally the time zone is assumed
/// to be UTC, which is also used implicitly for JSON serialization.
///
/// All constructors and conversions are provided as explicit
/// functions and not by implementing any `From`/`Into` traits
/// to prevent unintended usage.
#[derive(Clone, Default, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTime {
    // Timestamp in microseconds.
    pub(crate) timestamp_micros: i64,
}

impl From<columnar::DateTime> for DateTime {
    fn from(columnar_datetime: columnar::DateTime) -> Self {
        DateTime {
            timestamp_micros: columnar_datetime.timestamp_micros,
        }
    }
}

impl From<DateTime> for columnar::DateTime {
    fn from(datetime: crate::DateTime) -> Self {
        columnar::DateTime {
            timestamp_micros: datetime.timestamp_micros,
        }
    }
}

impl DateTime {
    /// Create new from UNIX timestamp in seconds
    pub const fn from_timestamp_secs(seconds: i64) -> Self {
        Self {
            timestamp_micros: seconds * 1_000_000,
        }
    }

    /// Create new from UNIX timestamp in milliseconds
    pub const fn from_timestamp_millis(milliseconds: i64) -> Self {
        Self {
            timestamp_micros: milliseconds * 1_000,
        }
    }

    /// Create new from UNIX timestamp in microseconds.
    pub const fn from_timestamp_micros(microseconds: i64) -> Self {
        Self {
            timestamp_micros: microseconds,
        }
    }

    /// Create new from `OffsetDateTime`
    ///
    /// The given date/time is converted to UTC and the actual
    /// time zone is discarded.
    pub const fn from_utc(dt: OffsetDateTime) -> Self {
        let timestamp_micros = dt.unix_timestamp() * 1_000_000 + dt.microsecond() as i64;
        Self { timestamp_micros }
    }

    /// Create new from `PrimitiveDateTime`
    ///
    /// Implicitly assumes that the given date/time is in UTC!
    /// Otherwise the original value must only be reobtained with
    /// [`Self::into_primitive()`].
    pub fn from_primitive(dt: PrimitiveDateTime) -> Self {
        Self::from_utc(dt.assume_utc())
    }

    /// Convert to UNIX timestamp in seconds.
    pub const fn into_timestamp_secs(self) -> i64 {
        self.timestamp_micros / 1_000_000
    }

    /// Convert to UNIX timestamp in milliseconds.
    pub const fn into_timestamp_millis(self) -> i64 {
        self.timestamp_micros / 1_000
    }

    /// Convert to UNIX timestamp in microseconds.
    pub const fn into_timestamp_micros(self) -> i64 {
        self.timestamp_micros
    }

    /// Convert to UTC `OffsetDateTime`
    pub fn into_utc(self) -> OffsetDateTime {
        let timestamp_nanos = self.timestamp_micros as i128 * 1000;
        let utc_datetime = OffsetDateTime::from_unix_timestamp_nanos(timestamp_nanos)
            .expect("valid UNIX timestamp");
        debug_assert_eq!(UtcOffset::UTC, utc_datetime.offset());
        utc_datetime
    }

    /// Convert to `OffsetDateTime` with the given time zone
    pub fn into_offset(self, offset: UtcOffset) -> OffsetDateTime {
        self.into_utc().to_offset(offset)
    }

    /// Convert to `PrimitiveDateTime` without any time zone
    ///
    /// The value should have been constructed with [`Self::from_primitive()`].
    /// Otherwise the time zone is implicitly assumed to be UTC.
    pub fn into_primitive(self) -> PrimitiveDateTime {
        let utc_datetime = self.into_utc();
        // Discard the UTC time zone offset
        debug_assert_eq!(UtcOffset::UTC, utc_datetime.offset());
        PrimitiveDateTime::new(utc_datetime.date(), utc_datetime.time())
    }

    /// Truncates the microseconds value to the corresponding precision.
    pub(crate) fn truncate(self, precision: DatePrecision) -> Self {
        let truncated_timestamp_micros = match precision {
            DatePrecision::Seconds => (self.timestamp_micros / 1_000_000) * 1_000_000,
            DatePrecision::Milliseconds => (self.timestamp_micros / 1_000) * 1_000,
            DatePrecision::Microseconds => self.timestamp_micros,
        };
        Self {
            timestamp_micros: truncated_timestamp_micros,
        }
    }
}

impl fmt::Debug for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let utc_rfc3339 = self.into_utc().format(&Rfc3339).map_err(|_| fmt::Error)?;
        f.write_str(&utc_rfc3339)
    }
}

pub use crate::error::TantivyError;
pub use crate::future_result::FutureResult;

/// Tantivy result.
///
/// Within tantivy, please avoid importing `Result` using `use crate::Result`
/// and instead, refer to this as `crate::Result<T>`.
pub type Result<T> = std::result::Result<T, TantivyError>;

mod core;
mod indexer;

#[allow(unused_doc_comments)]
pub mod error;
pub mod tokenizer;

pub mod aggregation;
pub mod collector;
pub mod directory;
pub mod fastfield;
pub mod fieldnorm;
pub mod positions;
pub mod postings;

/// Module containing the different query implementations.
pub mod query;
pub mod schema;
pub mod space_usage;
pub mod store;
pub mod termdict;

mod reader;

pub use self::reader::{IndexReader, IndexReaderBuilder, ReloadPolicy, Warmer};
mod snippet;
pub use self::snippet::{Snippet, SnippetGenerator};

mod docset;
use std::fmt;

pub use census::{Inventory, TrackedObject};
pub use common::{f64_to_u64, i64_to_u64, u64_to_f64, u64_to_i64, HasLen};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

pub use self::docset::{DocSet, TERMINATED};
pub use crate::core::{
    Executor, Index, IndexBuilder, IndexMeta, IndexSettings, IndexSortByField, InvertedIndexReader,
    Order, Searcher, SearcherGeneration, Segment, SegmentComponent, SegmentId, SegmentMeta,
    SegmentReader, SingleSegmentIndexWriter,
};
pub use crate::directory::Directory;
pub use crate::indexer::operation::UserOperation;
pub use crate::indexer::{merge_filtered_segments, merge_indices, IndexWriter, PreparedCommit};
pub use crate::postings::Postings;
pub use crate::schema::{DateOptions, DatePrecision, Document, Term};

/// Index format version.
const INDEX_FORMAT_VERSION: u32 = 5;

/// Structure version for the index.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    index_format_version: u32,
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

static VERSION: Lazy<Version> = Lazy::new(|| Version {
    major: env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap(),
    minor: env!("CARGO_PKG_VERSION_MINOR").parse().unwrap(),
    patch: env!("CARGO_PKG_VERSION_PATCH").parse().unwrap(),
    index_format_version: INDEX_FORMAT_VERSION,
});

impl ToString for Version {
    fn to_string(&self) -> String {
        format!(
            "tantivy v{}.{}.{}, index_format v{}",
            self.major, self.minor, self.patch, self.index_format_version
        )
    }
}

static VERSION_STRING: Lazy<String> = Lazy::new(|| VERSION.to_string());

/// Expose the current version of tantivy as found in Cargo.toml during compilation.
/// eg. "0.11.0" as well as the compression scheme used in the docstore.
pub fn version() -> &'static Version {
    &VERSION
}

/// Exposes the complete version of tantivy as found in Cargo.toml during compilation as a string.
/// eg. "tantivy v0.11.0, index_format v1, store_compression: lz4".
pub fn version_string() -> &'static str {
    VERSION_STRING.as_str()
}

/// Defines tantivy's merging strategy
pub mod merge_policy {
    pub use crate::indexer::{
        DefaultMergePolicy, LogMergePolicy, MergeCandidate, MergePolicy, NoMergePolicy,
    };
}

/// A `u32` identifying a document within a segment.
/// Documents have their `DocId` assigned incrementally,
/// as they are added in the segment.
///
/// At most, a segment can contain 2^31 documents.
pub type DocId = u32;

/// A u64 assigned to every operation incrementally
///
/// All operations modifying the index receives an monotonic Opstamp.
/// The resulting state of the index is consistent with the opstamp ordering.
///
/// For instance, a commit with opstamp `32_423` will reflect all Add and Delete operations
/// with an opstamp `<= 32_423`. A delete operation with opstamp n will no affect a document added
/// with opstamp `n+1`.
pub type Opstamp = u64;

/// A Score that represents the relevance of the document to the query
///
/// This is modelled internally as a `f32`. The larger the number, the more relevant
/// the document to the search query.
pub type Score = f32;

/// A `SegmentOrdinal` identifies a segment, within a `Searcher` or `Merger`.
pub type SegmentOrdinal = u32;

impl DocAddress {
    /// Creates a new DocAddress from the segment/docId pair.
    pub fn new(segment_ord: SegmentOrdinal, doc_id: DocId) -> DocAddress {
        DocAddress {
            segment_ord,
            doc_id,
        }
    }
}

/// `DocAddress` contains all the necessary information
/// to identify a document given a `Searcher` object.
///
/// It consists of an id identifying its segment, and
/// a segment-local `DocId`.
///
/// The id used for the segment is actually an ordinal
/// in the list of `Segment`s held by a `Searcher`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocAddress {
    /// The segment ordinal id that identifies the segment
    /// hosting the document in the `Searcher` it is called from.
    pub segment_ord: SegmentOrdinal,
    /// The segment-local `DocId`.
    pub doc_id: DocId,
}

#[cfg(test)]
pub mod tests {
    use common::{BinarySerializable, FixedSize};
    use rand::distributions::{Bernoulli, Uniform};
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    use time::OffsetDateTime;

    use crate::collector::tests::TEST_COLLECTOR_WITH_SCORE;
    use crate::core::SegmentReader;
    use crate::docset::{DocSet, TERMINATED};
    use crate::merge_policy::NoMergePolicy;
    use crate::query::BooleanQuery;
    use crate::schema::*;
    use crate::{DateTime, DocAddress, Index, Postings, ReloadPolicy};

    pub fn fixed_size_test<O: BinarySerializable + FixedSize + Default>() {
        let mut buffer = Vec::new();
        O::default().serialize(&mut buffer).unwrap();
        assert_eq!(buffer.len(), O::SIZE_IN_BYTES);
    }

    /// Checks if left and right are close one to each other.
    /// Panics if the two values are more than 0.5% apart.
    #[macro_export]
    macro_rules! assert_nearly_equals {
        ($left:expr, $right:expr) => {{
            match (&$left, &$right) {
                (left_val, right_val) => {
                    let diff = (left_val - right_val).abs();
                    let add = left_val.abs() + right_val.abs();
                    if diff > 0.0005 * add {
                        panic!(
                            r#"assertion failed: `(left ~= right)`
  left: `{:?}`,
 right: `{:?}`"#,
                            &*left_val, &*right_val
                        )
                    }
                }
            }
        }};
    }

    pub fn generate_nonunique_unsorted(max_value: u32, n_elems: usize) -> Vec<u32> {
        let seed: [u8; 32] = [1; 32];
        StdRng::from_seed(seed)
            .sample_iter(&Uniform::new(0u32, max_value))
            .take(n_elems)
            .collect::<Vec<u32>>()
    }

    pub fn sample_with_seed(n: u32, ratio: f64, seed_val: u8) -> Vec<u32> {
        StdRng::from_seed([seed_val; 32])
            .sample_iter(&Bernoulli::new(ratio).unwrap())
            .take(n as usize)
            .enumerate()
            .filter_map(|(val, keep)| if keep { Some(val as u32) } else { None })
            .collect()
    }

    pub fn sample(n: u32, ratio: f64) -> Vec<u32> {
        sample_with_seed(n, ratio, 4)
    }

    #[test]
    #[cfg(not(feature = "lz4"))]
    fn test_version_string() {
        use regex::Regex;
        let regex_ptn = Regex::new(
            "tantivy v[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}\\.{0,10}, index_format v[0-9]{1,5}",
        )
        .unwrap();
        let version = super::version().to_string();
        assert!(regex_ptn.find(&version).is_some());
    }

    #[test]
    #[cfg(feature = "mmap")]
    fn test_indexing() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_from_tempdir(schema)?;
        // writing the segment
        let mut index_writer = index.writer_for_tests()?;
        {
            let doc = doc!(text_field=>"af b");
            index_writer.add_document(doc)?;
        }
        {
            let doc = doc!(text_field=>"a b c");
            index_writer.add_document(doc)?;
        }
        {
            let doc = doc!(text_field=>"a b c d");
            index_writer.add_document(doc)?;
        }
        index_writer.commit()?;
        Ok(())
    }

    #[test]
    fn test_docfreq1() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema_builder.build());
        let mut index_writer = index.writer_for_tests()?;
        index_writer.add_document(doc!(text_field=>"a b c"))?;
        index_writer.commit()?;
        index_writer.add_document(doc!(text_field=>"a"))?;
        index_writer.add_document(doc!(text_field=>"a a"))?;
        index_writer.commit()?;
        index_writer.add_document(doc!(text_field=>"c"))?;
        index_writer.commit()?;
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let term_a = Term::from_field_text(text_field, "a");
        assert_eq!(searcher.doc_freq(&term_a)?, 3);
        let term_b = Term::from_field_text(text_field, "b");
        assert_eq!(searcher.doc_freq(&term_b)?, 1);
        let term_c = Term::from_field_text(text_field, "c");
        assert_eq!(searcher.doc_freq(&term_c)?, 2);
        let term_d = Term::from_field_text(text_field, "d");
        assert_eq!(searcher.doc_freq(&term_d)?, 0);
        Ok(())
    }

    #[test]
    fn test_fieldnorm_no_docs_with_field() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let title_field = schema_builder.add_text_field("title", TEXT);
        let text_field = schema_builder.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema_builder.build());
        let mut index_writer = index.writer_for_tests()?;
        index_writer.add_document(doc!(text_field=>"a b c"))?;
        index_writer.commit()?;
        let index_reader = index.reader()?;
        let searcher = index_reader.searcher();
        let reader = searcher.segment_reader(0);
        {
            let fieldnorm_reader = reader.get_fieldnorms_reader(text_field)?;
            assert_eq!(fieldnorm_reader.fieldnorm(0), 3);
        }
        {
            let fieldnorm_reader = reader.get_fieldnorms_reader(title_field)?;
            assert_eq!(fieldnorm_reader.fieldnorm_id(0), 0);
        }
        Ok(())
    }

    #[test]
    fn test_fieldnorm() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema_builder.build());
        let mut index_writer = index.writer_for_tests()?;
        index_writer.add_document(doc!(text_field=>"a b c"))?;
        index_writer.add_document(doc!())?;
        index_writer.add_document(doc!(text_field=>"a b"))?;
        index_writer.commit()?;
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let segment_reader: &SegmentReader = searcher.segment_reader(0);
        let fieldnorms_reader = segment_reader.get_fieldnorms_reader(text_field)?;
        assert_eq!(fieldnorms_reader.fieldnorm(0), 3);
        assert_eq!(fieldnorms_reader.fieldnorm(1), 0);
        assert_eq!(fieldnorms_reader.fieldnorm(2), 2);
        Ok(())
    }

    fn advance_undeleted(docset: &mut dyn DocSet, reader: &SegmentReader) -> bool {
        let mut doc = docset.advance();
        while doc != TERMINATED {
            if !reader.is_deleted(doc) {
                return true;
            }
            doc = docset.advance();
        }
        false
    }

    #[test]
    fn test_delete_postings1() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let term_abcd = Term::from_field_text(text_field, "abcd");
        let term_a = Term::from_field_text(text_field, "a");
        let term_b = Term::from_field_text(text_field, "b");
        let term_c = Term::from_field_text(text_field, "c");
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .unwrap();
        {
            // writing the segment
            let mut index_writer = index.writer_for_tests()?;
            // 0
            index_writer.add_document(doc!(text_field=>"a b"))?;
            // 1
            index_writer.add_document(doc!(text_field=>" a c"))?;
            // 2
            index_writer.add_document(doc!(text_field=>" b c"))?;
            // 3
            index_writer.add_document(doc!(text_field=>" b d"))?;

            index_writer.delete_term(Term::from_field_text(text_field, "c"));
            index_writer.delete_term(Term::from_field_text(text_field, "a"));
            // 4
            index_writer.add_document(doc!(text_field=>" b c"))?;
            // 5
            index_writer.add_document(doc!(text_field=>" a"))?;
            index_writer.commit()?;
        }
        {
            reader.reload()?;
            let searcher = reader.searcher();
            let segment_reader = searcher.segment_reader(0);
            let inverted_index = segment_reader.inverted_index(text_field)?;
            assert!(inverted_index
                .read_postings(&term_abcd, IndexRecordOption::WithFreqsAndPositions)?
                .is_none());
            {
                let mut postings = inverted_index
                    .read_postings(&term_a, IndexRecordOption::WithFreqsAndPositions)?
                    .unwrap();
                assert!(advance_undeleted(&mut postings, segment_reader));
                assert_eq!(postings.doc(), 5);
                assert!(!advance_undeleted(&mut postings, segment_reader));
            }
            {
                let mut postings = inverted_index
                    .read_postings(&term_b, IndexRecordOption::WithFreqsAndPositions)?
                    .unwrap();
                assert!(advance_undeleted(&mut postings, segment_reader));
                assert_eq!(postings.doc(), 3);
                assert!(advance_undeleted(&mut postings, segment_reader));
                assert_eq!(postings.doc(), 4);
                assert!(!advance_undeleted(&mut postings, segment_reader));
            }
        }
        {
            // writing the segment
            let mut index_writer = index.writer_for_tests()?;
            // 0
            index_writer.add_document(doc!(text_field=>"a b"))?;
            // 1
            index_writer.delete_term(Term::from_field_text(text_field, "c"));
            index_writer.rollback()?;
        }
        {
            reader.reload()?;
            let searcher = reader.searcher();
            let seg_reader = searcher.segment_reader(0);
            let inverted_index = seg_reader.inverted_index(term_abcd.field())?;

            assert!(inverted_index
                .read_postings(&term_abcd, IndexRecordOption::WithFreqsAndPositions)?
                .is_none());
            {
                let mut postings = inverted_index
                    .read_postings(&term_a, IndexRecordOption::WithFreqsAndPositions)?
                    .unwrap();
                assert!(advance_undeleted(&mut postings, seg_reader));
                assert_eq!(postings.doc(), 5);
                assert!(!advance_undeleted(&mut postings, seg_reader));
            }
            {
                let mut postings = inverted_index
                    .read_postings(&term_b, IndexRecordOption::WithFreqsAndPositions)?
                    .unwrap();
                assert!(advance_undeleted(&mut postings, seg_reader));
                assert_eq!(postings.doc(), 3);
                assert!(advance_undeleted(&mut postings, seg_reader));
                assert_eq!(postings.doc(), 4);
                assert!(!advance_undeleted(&mut postings, seg_reader));
            }
        }
        {
            // writing the segment
            let mut index_writer = index.writer_for_tests()?;
            index_writer.add_document(doc!(text_field=>"a b"))?;
            index_writer.delete_term(Term::from_field_text(text_field, "c"));
            index_writer.rollback()?;
            index_writer.delete_term(Term::from_field_text(text_field, "a"));
            index_writer.commit()?;
        }
        {
            reader.reload()?;
            let searcher = reader.searcher();
            let segment_reader = searcher.segment_reader(0);
            let inverted_index = segment_reader.inverted_index(term_abcd.field())?;
            assert!(inverted_index
                .read_postings(&term_abcd, IndexRecordOption::WithFreqsAndPositions)?
                .is_none());
            {
                let mut postings = inverted_index
                    .read_postings(&term_a, IndexRecordOption::WithFreqsAndPositions)?
                    .unwrap();
                assert!(!advance_undeleted(&mut postings, segment_reader));
            }
            {
                let mut postings = inverted_index
                    .read_postings(&term_b, IndexRecordOption::WithFreqsAndPositions)?
                    .unwrap();
                assert!(advance_undeleted(&mut postings, segment_reader));
                assert_eq!(postings.doc(), 3);
                assert!(advance_undeleted(&mut postings, segment_reader));
                assert_eq!(postings.doc(), 4);
                assert!(!advance_undeleted(&mut postings, segment_reader));
            }
            {
                let mut postings = inverted_index
                    .read_postings(&term_c, IndexRecordOption::WithFreqsAndPositions)?
                    .unwrap();
                assert!(advance_undeleted(&mut postings, segment_reader));
                assert_eq!(postings.doc(), 4);
                assert!(!advance_undeleted(&mut postings, segment_reader));
            }
        }
        Ok(())
    }

    #[test]
    fn test_indexed_u64() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let field = schema_builder.add_u64_field("value", INDEXED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        let mut index_writer = index.writer_for_tests()?;
        index_writer.add_document(doc!(field=>1u64))?;
        index_writer.commit()?;
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let term = Term::from_field_u64(field, 1u64);
        let mut postings = searcher
            .segment_reader(0)
            .inverted_index(term.field())?
            .read_postings(&term, IndexRecordOption::Basic)?
            .unwrap();
        assert_eq!(postings.doc(), 0);
        assert_eq!(postings.advance(), TERMINATED);
        Ok(())
    }

    #[test]
    fn test_indexed_i64() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let value_field = schema_builder.add_i64_field("value", INDEXED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        let mut index_writer = index.writer_for_tests()?;
        let negative_val = -1i64;
        index_writer.add_document(doc!(value_field => negative_val))?;
        index_writer.commit()?;
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let term = Term::from_field_i64(value_field, negative_val);
        let mut postings = searcher
            .segment_reader(0)
            .inverted_index(term.field())?
            .read_postings(&term, IndexRecordOption::Basic)?
            .unwrap();
        assert_eq!(postings.doc(), 0);
        assert_eq!(postings.advance(), TERMINATED);
        Ok(())
    }

    #[test]
    fn test_indexed_f64() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let value_field = schema_builder.add_f64_field("value", INDEXED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        let mut index_writer = index.writer_for_tests()?;
        let val = std::f64::consts::PI;
        index_writer.add_document(doc!(value_field => val))?;
        index_writer.commit()?;
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let term = Term::from_field_f64(value_field, val);
        let mut postings = searcher
            .segment_reader(0)
            .inverted_index(term.field())?
            .read_postings(&term, IndexRecordOption::Basic)?
            .unwrap();
        assert_eq!(postings.doc(), 0);
        assert_eq!(postings.advance(), TERMINATED);
        Ok(())
    }

    #[test]
    fn test_indexedfield_not_in_documents() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let absent_field = schema_builder.add_text_field("absent_text", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let mut index_writer = index.writer_for_tests()?;
        index_writer.add_document(doc!(text_field=>"a"))?;
        assert!(index_writer.commit().is_ok());
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let segment_reader = searcher.segment_reader(0);
        let inverted_index = segment_reader.inverted_index(absent_field)?;
        assert_eq!(inverted_index.terms().num_terms(), 0);
        Ok(())
    }

    #[test]
    fn test_delete_postings2() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        // writing the segment
        let mut index_writer = index.writer_for_tests()?;
        index_writer.add_document(doc!(text_field=>"63"))?;
        index_writer.add_document(doc!(text_field=>"70"))?;
        index_writer.add_document(doc!(text_field=>"34"))?;
        index_writer.add_document(doc!(text_field=>"1"))?;
        index_writer.add_document(doc!(text_field=>"38"))?;
        index_writer.add_document(doc!(text_field=>"33"))?;
        index_writer.add_document(doc!(text_field=>"40"))?;
        index_writer.add_document(doc!(text_field=>"17"))?;
        index_writer.delete_term(Term::from_field_text(text_field, "38"));
        index_writer.delete_term(Term::from_field_text(text_field, "34"));
        index_writer.commit()?;
        reader.reload()?;
        assert_eq!(reader.searcher().num_docs(), 6);
        Ok(())
    }

    #[test]
    fn test_termfreq() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        {
            // writing the segment
            let mut index_writer = index.writer_for_tests()?;
            index_writer.add_document(doc!(text_field=>"af af af bc bc"))?;
            index_writer.commit()?;
        }
        {
            let index_reader = index.reader()?;
            let searcher = index_reader.searcher();
            let reader = searcher.segment_reader(0);
            let inverted_index = reader.inverted_index(text_field)?;
            let term_abcd = Term::from_field_text(text_field, "abcd");
            assert!(inverted_index
                .read_postings(&term_abcd, IndexRecordOption::WithFreqsAndPositions)?
                .is_none());
            let term_af = Term::from_field_text(text_field, "af");
            let mut postings = inverted_index
                .read_postings(&term_af, IndexRecordOption::WithFreqsAndPositions)?
                .unwrap();
            assert_eq!(postings.doc(), 0);
            assert_eq!(postings.term_freq(), 3);
            assert_eq!(postings.advance(), TERMINATED);
        }
        Ok(())
    }

    #[test]
    fn test_searcher_1() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let reader = index.reader()?;
        // writing the segment
        let mut index_writer = index.writer_for_tests()?;
        index_writer.add_document(doc!(text_field=>"af af af b"))?;
        index_writer.add_document(doc!(text_field=>"a b c"))?;
        index_writer.add_document(doc!(text_field=>"a b c d"))?;
        index_writer.commit()?;

        reader.reload()?;
        let searcher = reader.searcher();
        let get_doc_ids = |terms: Vec<Term>| {
            let query = BooleanQuery::new_multiterms_query(terms);
            searcher
                .search(&query, &TEST_COLLECTOR_WITH_SCORE)
                .map(|topdocs| topdocs.docs().to_vec())
        };
        assert_eq!(
            get_doc_ids(vec![Term::from_field_text(text_field, "a")])?,
            vec![DocAddress::new(0, 1), DocAddress::new(0, 2)]
        );
        assert_eq!(
            get_doc_ids(vec![Term::from_field_text(text_field, "af")])?,
            vec![DocAddress::new(0, 0)]
        );
        assert_eq!(
            get_doc_ids(vec![Term::from_field_text(text_field, "b")])?,
            vec![
                DocAddress::new(0, 0),
                DocAddress::new(0, 1),
                DocAddress::new(0, 2)
            ]
        );
        assert_eq!(
            get_doc_ids(vec![Term::from_field_text(text_field, "c")])?,
            vec![DocAddress::new(0, 1), DocAddress::new(0, 2)]
        );
        assert_eq!(
            get_doc_ids(vec![Term::from_field_text(text_field, "d")])?,
            vec![DocAddress::new(0, 2)]
        );
        assert_eq!(
            get_doc_ids(vec![
                Term::from_field_text(text_field, "b"),
                Term::from_field_text(text_field, "a"),
            ])?,
            vec![
                DocAddress::new(0, 0),
                DocAddress::new(0, 1),
                DocAddress::new(0, 2)
            ]
        );
        Ok(())
    }

    #[test]
    fn test_searcher_2() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        assert_eq!(reader.searcher().num_docs(), 0u64);
        // writing the segment
        let mut index_writer = index.writer_for_tests()?;
        index_writer.add_document(doc!(text_field=>"af b"))?;
        index_writer.add_document(doc!(text_field=>"a b c"))?;
        index_writer.add_document(doc!(text_field=>"a b c d"))?;
        index_writer.commit()?;
        reader.reload()?;
        assert_eq!(reader.searcher().num_docs(), 3u64);
        Ok(())
    }

    #[test]
    fn test_doc_macro() {
        let mut schema_builder = Schema::builder();
        let text_field = schema_builder.add_text_field("text", TEXT);
        let other_text_field = schema_builder.add_text_field("text2", TEXT);
        let document = doc!(text_field => "tantivy",
                            text_field => "some other value",
                            other_text_field => "short");
        assert_eq!(document.len(), 3);
        let values: Vec<&Value> = document.get_all(text_field).collect();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].as_text(), Some("tantivy"));
        assert_eq!(values[1].as_text(), Some("some other value"));
        let values: Vec<&Value> = document.get_all(other_text_field).collect();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].as_text(), Some("short"));
    }

    #[test]
    fn test_wrong_fast_field_type() -> crate::Result<()> {
        let mut schema_builder = Schema::builder();
        let fast_field_unsigned = schema_builder.add_u64_field("unsigned", FAST);
        let fast_field_signed = schema_builder.add_i64_field("signed", FAST);
        let fast_field_float = schema_builder.add_f64_field("float", FAST);
        schema_builder.add_text_field("text", TEXT);
        schema_builder.add_u64_field("stored_int", STORED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        let mut index_writer = index.writer_for_tests()?;
        {
            let document =
                doc!(fast_field_unsigned => 4u64, fast_field_signed=>4i64, fast_field_float=>4f64);
            index_writer.add_document(document)?;
            index_writer.commit()?;
        }
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let segment_reader: &SegmentReader = searcher.segment_reader(0);
        {
            let fast_field_reader_res = segment_reader.fast_fields().u64("text");
            assert!(fast_field_reader_res.is_err());
        }
        {
            let fast_field_reader_opt = segment_reader.fast_fields().u64("stored_int");
            assert!(fast_field_reader_opt.is_err());
        }
        {
            let fast_field_reader_opt = segment_reader.fast_fields().u64("signed");
            assert!(fast_field_reader_opt.is_err());
        }
        {
            let fast_field_reader_opt = segment_reader.fast_fields().u64("float");
            assert!(fast_field_reader_opt.is_err());
        }
        {
            let fast_field_reader_opt = segment_reader.fast_fields().u64("unsigned");
            assert!(fast_field_reader_opt.is_ok());
            let fast_field_reader = fast_field_reader_opt.unwrap();
            assert_eq!(fast_field_reader.get_val(0), 4u64)
        }

        {
            let fast_field_reader_res = segment_reader.fast_fields().i64("signed");
            assert!(fast_field_reader_res.is_ok());
            let fast_field_reader = fast_field_reader_res.unwrap();
            assert_eq!(fast_field_reader.get_val(0), 4i64)
        }

        {
            let fast_field_reader_res = segment_reader.fast_fields().f64("float");
            assert!(fast_field_reader_res.is_ok());
            let fast_field_reader = fast_field_reader_res.unwrap();
            assert_eq!(fast_field_reader.get_val(0), 4f64)
        }
        Ok(())
    }

    // motivated by #729
    #[test]
    fn test_update_via_delete_insert() -> crate::Result<()> {
        use crate::collector::Count;
        use crate::indexer::NoMergePolicy;
        use crate::query::AllQuery;
        use crate::SegmentId;

        const DOC_COUNT: u64 = 2u64;

        let mut schema_builder = SchemaBuilder::default();
        let id = schema_builder.add_u64_field("id", INDEXED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        let index_reader = index.reader()?;

        let mut index_writer = index.writer_for_tests()?;
        index_writer.set_merge_policy(Box::new(NoMergePolicy));

        for doc_id in 0u64..DOC_COUNT {
            index_writer.add_document(doc!(id => doc_id))?;
        }
        index_writer.commit()?;

        index_reader.reload()?;
        let searcher = index_reader.searcher();

        assert_eq!(
            searcher.search(&AllQuery, &Count).unwrap(),
            DOC_COUNT as usize
        );

        // update the 10 elements by deleting and re-adding
        for doc_id in 0u64..DOC_COUNT {
            index_writer.delete_term(Term::from_field_u64(id, doc_id));
            index_writer.commit()?;
            index_reader.reload()?;
            index_writer.add_document(doc!(id =>  doc_id))?;
            index_writer.commit()?;
            index_reader.reload()?;
            let searcher = index_reader.searcher();
            // The number of document should be stable.
            assert_eq!(
                searcher.search(&AllQuery, &Count).unwrap(),
                DOC_COUNT as usize
            );
        }

        index_reader.reload()?;
        let searcher = index_reader.searcher();
        let segment_ids: Vec<SegmentId> = searcher
            .segment_readers()
            .iter()
            .map(|reader| reader.segment_id())
            .collect();
        index_writer.merge(&segment_ids).wait()?;
        index_reader.reload()?;
        let searcher = index_reader.searcher();
        assert_eq!(searcher.search(&AllQuery, &Count)?, DOC_COUNT as usize);
        Ok(())
    }

    #[test]
    fn test_validate_checksum() -> crate::Result<()> {
        let index_path = tempfile::tempdir().expect("dir");
        let mut builder = Schema::builder();
        let body = builder.add_text_field("body", TEXT | STORED);
        let schema = builder.build();
        let index = Index::create_in_dir(&index_path, schema)?;
        let mut writer = index.writer(50_000_000)?;
        writer.set_merge_policy(Box::new(NoMergePolicy));
        for _ in 0..5000 {
            writer.add_document(doc!(body => "foo"))?;
            writer.add_document(doc!(body => "boo"))?;
        }
        writer.commit()?;
        assert!(index.validate_checksum()?.is_empty());

        // delete few docs
        writer.delete_term(Term::from_field_text(body, "foo"));
        writer.commit()?;
        let segment_ids = index.searchable_segment_ids()?;
        writer.merge(&segment_ids).wait()?;
        assert!(index.validate_checksum()?.is_empty());
        Ok(())
    }

    #[test]
    fn test_datetime() {
        let now = OffsetDateTime::now_utc();

        let dt = DateTime::from_utc(now).into_utc();
        assert_eq!(dt.to_ordinal_date(), now.to_ordinal_date());
        assert_eq!(dt.to_hms_micro(), now.to_hms_micro());
        // We don't store nanosecond level precision.
        assert_eq!(dt.nanosecond(), now.microsecond() * 1000);

        let dt = DateTime::from_timestamp_secs(now.unix_timestamp()).into_utc();
        assert_eq!(dt.to_ordinal_date(), now.to_ordinal_date());
        assert_eq!(dt.to_hms(), now.to_hms());
        // Constructed from a second precision.
        assert_ne!(dt.to_hms_micro(), now.to_hms_micro());

        let dt =
            DateTime::from_timestamp_micros((now.unix_timestamp_nanos() / 1_000) as i64).into_utc();
        assert_eq!(dt.to_ordinal_date(), now.to_ordinal_date());
        assert_eq!(dt.to_hms_micro(), now.to_hms_micro());

        let dt_from_ts_nanos =
            OffsetDateTime::from_unix_timestamp_nanos(18446744073709551615i128).unwrap();
        let offset_dt = DateTime::from_utc(dt_from_ts_nanos).into_utc();
        assert_eq!(
            dt_from_ts_nanos.to_ordinal_date(),
            offset_dt.to_ordinal_date()
        );
        assert_eq!(dt_from_ts_nanos.to_hms_micro(), offset_dt.to_hms_micro());
    }
}
