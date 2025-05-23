use std::io;
use std::io::Write;
use std::ops::Range;
use std::sync::Arc;

use common::OwnedBytes;

use crate::column_values::u64_based::CodecType;
use crate::column_values::ColumnValues;
use crate::iterable::Iterable;
use crate::RowId;

pub fn serialize_multivalued_index(
    multivalued_index: &dyn Iterable<RowId>,
    output: &mut impl Write,
) -> io::Result<()> {
    crate::column_values::u64_based::serialize_u64_based_column_values(
        multivalued_index,
        &[CodecType::Bitpacked, CodecType::Linear],
        output,
    )?;
    Ok(())
}

pub fn open_multivalued_index(bytes: OwnedBytes) -> io::Result<MultiValueIndex> {
    let start_index_column: Arc<dyn ColumnValues<RowId>> =
        crate::column_values::u64_based::load_u64_based_column_values(bytes)?;
    Ok(MultiValueIndex { start_index_column })
}

#[derive(Clone)]
/// Index to resolve value range for given doc_id.
/// Starts at 0.
pub struct MultiValueIndex {
    pub start_index_column: Arc<dyn crate::ColumnValues<RowId>>,
}

impl From<Arc<dyn ColumnValues<RowId>>> for MultiValueIndex {
    fn from(start_index_column: Arc<dyn ColumnValues<RowId>>) -> Self {
        MultiValueIndex { start_index_column }
    }
}

impl MultiValueIndex {
    pub fn for_test(start_offsets: &[RowId]) -> MultiValueIndex {
        let mut buffer = Vec::new();
        serialize_multivalued_index(&start_offsets, &mut buffer).unwrap();
        let bytes = OwnedBytes::new(buffer);
        open_multivalued_index(bytes).unwrap()
    }

    /// Returns `[start, end)`, such that the values associated with
    /// the given document are `start..end`.
    #[inline]
    pub(crate) fn range(&self, row_id: RowId) -> Range<RowId> {
        let start = self.start_index_column.get_val(row_id);
        let end = self.start_index_column.get_val(row_id + 1);
        start..end
    }

    /// Returns the number of documents in the index.
    #[inline]
    pub fn num_rows(&self) -> u32 {
        self.start_index_column.num_vals() - 1
    }

    /// Converts a list of ranks (row ids of values) in a 1:n index to the corresponding list of
    /// row_ids. Positions are converted inplace to docids.
    ///
    /// Since there is no index for value pos -> docid, but docid -> value pos range, we scan the
    /// index.
    ///
    /// Correctness: positions needs to be sorted. idx_reader needs to contain monotonically
    /// increasing positions.
    ///
    /// TODO: Instead of a linear scan we can employ a exponential search into binary search to
    /// match a docid to its value position.
    #[allow(clippy::bool_to_int_with_if)]
    pub(crate) fn select_batch_in_place(&self, row_start: RowId, ranks: &mut Vec<u32>) {
        if ranks.is_empty() {
            return;
        }
        let mut cur_doc = row_start;
        let mut last_doc = None;

        assert!(self.start_index_column.get_val(row_start) as u32 <= ranks[0]);

        let mut write_doc_pos = 0;
        for i in 0..ranks.len() {
            let pos = ranks[i];
            loop {
                let end = self.start_index_column.get_val(cur_doc + 1) as u32;
                if end > pos {
                    ranks[write_doc_pos] = cur_doc;
                    write_doc_pos += if last_doc == Some(cur_doc) { 0 } else { 1 };
                    last_doc = Some(cur_doc);
                    break;
                }
                cur_doc += 1;
            }
        }
        ranks.truncate(write_doc_pos);
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::sync::Arc;

    use super::MultiValueIndex;
    use crate::column_values::IterColumn;
    use crate::{ColumnValues, RowId};

    fn index_to_pos_helper(
        index: &MultiValueIndex,
        doc_id_range: Range<u32>,
        positions: &[u32],
    ) -> Vec<u32> {
        let mut positions = positions.to_vec();
        index.select_batch_in_place(doc_id_range.start, &mut positions);
        positions
    }

    #[test]
    fn test_positions_to_docid() {
        let offsets: Vec<RowId> = vec![0, 10, 12, 15, 22, 23]; // docid values are [0..10, 10..12, 12..15, etc.]
        let column: Arc<dyn ColumnValues<RowId>> = Arc::new(IterColumn::from(offsets.into_iter()));
        let index = MultiValueIndex::from(column);
        assert_eq!(index.num_rows(), 5);
        let positions = &[10u32, 11, 15, 20, 21, 22];
        assert_eq!(index_to_pos_helper(&index, 0..5, positions), vec![1, 3, 4]);
        assert_eq!(index_to_pos_helper(&index, 1..5, positions), vec![1, 3, 4]);
        assert_eq!(index_to_pos_helper(&index, 0..5, &[9]), vec![0]);
        assert_eq!(index_to_pos_helper(&index, 1..5, &[10]), vec![1]);
        assert_eq!(index_to_pos_helper(&index, 1..5, &[11]), vec![1]);
        assert_eq!(index_to_pos_helper(&index, 2..5, &[12]), vec![2]);
        assert_eq!(index_to_pos_helper(&index, 2..5, &[12, 14]), vec![2]);
        assert_eq!(index_to_pos_helper(&index, 2..5, &[12, 14, 15]), vec![2, 3]);
    }
}
