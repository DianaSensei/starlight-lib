use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use uuid::Uuid;

pub const DEFAULT_PAGE_LIMIT: u32 = 20;
pub const MAX_PAGE_LIMIT: u32 = 100;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaginationError {
    #[error("limit must be between 1 and {MAX_PAGE_LIMIT}")]
    InvalidLimit,
    #[error("cursor is malformed")]
    InvalidCursor,
}

/// Keyset page request ordered by id descending; UUIDv7 ids make this newest-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    limit: u32,
    after: Option<Uuid>,
}

impl PageRequest {
    pub fn new(limit: Option<u32>, cursor: Option<&str>) -> Result<Self, PaginationError> {
        let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        if limit == 0 || limit > MAX_PAGE_LIMIT {
            return Err(PaginationError::InvalidLimit);
        }
        let after = cursor.map(decode_cursor).transpose()?;
        Ok(Self { limit, after })
    }

    pub fn first(limit: u32) -> Self {
        Self {
            limit: limit.clamp(1, MAX_PAGE_LIMIT),
            after: None,
        }
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// Id of the last item of the previous page; results must be strictly older.
    pub fn after(&self) -> Option<Uuid> {
        self.after
    }

    /// One extra row tells whether another page exists without a COUNT query.
    pub fn fetch_limit(&self) -> u64 {
        u64::from(self.limit) + 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub limit: u32,
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    /// Builds a page from rows fetched with [`PageRequest::fetch_limit`].
    pub fn from_overfetched(
        mut rows: Vec<T>,
        request: &PageRequest,
        id_of: impl Fn(&T) -> Uuid,
    ) -> Self {
        let limit = request.limit as usize;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = rows
            .last()
            .filter(|_| has_more)
            .map(|last| encode_cursor(id_of(last)));
        Self {
            items: rows,
            limit: request.limit,
            next_cursor,
        }
    }

    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U> {
        Page {
            items: self.items.into_iter().map(f).collect(),
            limit: self.limit,
            next_cursor: self.next_cursor,
        }
    }
}

fn encode_cursor(id: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(id.as_bytes())
}

fn decode_cursor(cursor: &str) -> Result<Uuid, PaginationError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| PaginationError::InvalidCursor)?;
    Uuid::from_slice(&bytes).map_err(|_| PaginationError::InvalidCursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limit_applies_when_absent() {
        let request = PageRequest::new(None, None).unwrap();

        assert_eq!(request.limit(), DEFAULT_PAGE_LIMIT);
        assert_eq!(request.after(), None);
    }

    #[test]
    fn limit_outside_bounds_is_rejected() {
        assert_eq!(
            PageRequest::new(Some(0), None),
            Err(PaginationError::InvalidLimit)
        );
        assert_eq!(
            PageRequest::new(Some(MAX_PAGE_LIMIT + 1), None),
            Err(PaginationError::InvalidLimit)
        );
    }

    #[test]
    fn malformed_cursor_is_rejected() {
        assert_eq!(
            PageRequest::new(None, Some("not-a-cursor!")),
            Err(PaginationError::InvalidCursor)
        );
    }

    #[test]
    fn overfetched_rows_produce_cursor_that_round_trips() {
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
        let request = PageRequest::new(Some(2), None).unwrap();

        let page = Page::from_overfetched(ids.clone(), &request, |id| *id);
        let next = PageRequest::new(Some(2), page.next_cursor.as_deref()).unwrap();

        assert_eq!(page.items, ids[..2]);
        assert!(page.has_more());
        assert_eq!(next.after(), Some(ids[1]));
    }

    #[test]
    fn last_page_has_no_cursor() {
        let ids: Vec<Uuid> = (0..2).map(|_| Uuid::now_v7()).collect();
        let request = PageRequest::new(Some(2), None).unwrap();

        let page = Page::from_overfetched(ids, &request, |id| *id);

        assert!(!page.has_more());
    }
}
