/// Application transport adapter for basic-filter matching. The portable
/// compact_filters module remains unaware of frames, handles and managed code.
pub fn match_basic_compact_filter(request: &Frame) -> Frame {
    if request.operation != COMPACT_FILTER_MATCH_ANY {
        return request.error(1, "unexpected compact-filter operation");
    }
    if request.payload.len() > MAX_FRAME - HEADER {
        return request.error(1, "compact-filter request exceeds frame limit");
    }
    type FilterQuery<'a> = ([u8; 32], &'a [u8], Vec<&'a [u8]>);
    fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Result<&'a [u8], &'static str> {
        let value = bytes
            .get(..length)
            .ok_or("truncated compact-filter request")?;
        *bytes = &bytes[length..];
        Ok(value)
    }
    fn number(bytes: &mut &[u8]) -> Result<usize, &'static str> {
        let value: [u8; 4] = take(bytes, 4)?
            .try_into()
            .map_err(|_| "invalid compact-filter length")?;
        Ok(u32::from_le_bytes(value) as usize)
    }
    fn blob<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], &'static str> {
        let length = number(bytes)?;
        take(bytes, length)
    }
    fn decode(mut bytes: &[u8]) -> Result<FilterQuery<'_>, &'static str> {
        let hash = take(&mut bytes, 32)?
            .try_into()
            .map_err(|_| "invalid block hash")?;
        let filter = blob(&mut bytes)?;
        let count = number(&mut bytes)?;
        if count > 65_536 {
            return Err("too many compact-filter queries");
        }
        // Each query needs a four-byte length even when the script is empty.
        if count > bytes.len() / 4 {
            return Err("truncated compact-filter query list");
        }
        let mut queries = Vec::new();
        queries
            .try_reserve_exact(count)
            .map_err(|_| "compact-filter allocation failed")?;
        for _ in 0..count {
            queries.push(blob(&mut bytes)?);
        }
        if !bytes.is_empty() {
            return Err("trailing compact-filter request bytes");
        }
        Ok((hash, filter, queries))
    }
    let (hash, filter, queries) = match decode(&request.payload) {
        Ok(value) => value,
        Err(message) => return request.error(1, message),
    };
    let limits = crate::compact_filters::Limits {
        max_filter_bytes: MAX_FRAME - HEADER,
        max_elements: 1_000_000,
        max_queries: 65_536,
        max_input_bytes: MAX_FRAME - HEADER,
    };
    let result = crate::compact_filters::GcsFilter::parse_basic(filter, &hash, limits)
        .and_then(|filter| filter.match_any(&queries));
    match result {
        Ok(matched) => request.reply(vec![u8::from(matched)]),
        Err(error) => request.error(2, &error.to_string()),
    }
}
