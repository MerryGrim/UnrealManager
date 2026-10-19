//! Bounded access to install tags omitted by egs-api's public manifest model.
use sha1::{Digest, Sha1};
use std::{collections::BTreeMap, io::Read};
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("Размер манифеста")?;
        let v = self.data.get(self.pos..end).ok_or("Манифест обрезан")?;
        self.pos = end;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as i32;
        if n == 0 {
            return Ok(String::new());
        }
        let count = n.unsigned_abs() as usize;
        if count > 65536 {
            return Err("Строка манифеста слишком велика".into());
        }
        let raw = self.take(count * if n < 0 { 2 } else { 1 })?;
        if n < 0 {
            let units = raw
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>();
            if units.last() != Some(&0) {
                return Err("Строка не завершена".into());
            }
            String::from_utf16(&units[..units.len() - 1]).map_err(|_| "Некорректная строка".into())
        } else {
            if raw.last() != Some(&0) {
                return Err("Строка не завершена".into());
            }
            String::from_utf8(raw[..raw.len() - 1].to_vec())
                .map_err(|_| "Некорректная строка".into())
        }
    }
    fn skip_section(&mut self) -> Result<(), String> {
        let n = self.u32()? as usize;
        if n < 5 {
            return Err("Некорректная секция".into());
        }
        self.take(n - 4)?;
        Ok(())
    }
}
fn payload(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut h = Reader { data: raw, pos: 0 };
    if h.u32()? != 1153351692 {
        return Err("Нужен бинарный манифест Epic".into());
    }
    let header = h.u32()? as usize;
    let size = h.u32()? as usize;
    let packed = h.u32()? as usize;
    let hash = h.take(20)?.to_vec();
    let storage = h.take(1)?[0];
    let _version = h.u32()?;
    if header < 41
        || header > raw.len()
        || size > 512 * 1024 * 1024
        || packed != raw.len() - header
        || storage > 1
    {
        return Err("Некорректный заголовок манифеста".into());
    }
    let data = if storage == 1 {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&raw[header..])
            .take(size as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|_| "Ошибка распаковки манифеста")?;
        out
    } else {
        raw[header..].to_vec()
    };
    if data.len() != size || Sha1::digest(&data).as_slice() != hash {
        return Err("Контрольная сумма манифеста не совпала".into());
    }
    Ok(data)
}
pub(super) fn identity(raw: &[u8]) -> Result<(String, String), String> {
    let data = payload(raw)?;
    let mut r = Reader {
        data: &data,
        pos: 0,
    };
    let size = r.u32()? as usize;
    if size < 14 {
        return Err("Некорректные метаданные".into());
    }
    let mut r = Reader {
        data: r.take(size - 4)?,
        pos: 0,
    };
    r.take(10)?;
    Ok((r.string()?, r.string()?))
}
pub(super) fn tags(raw: &[u8]) -> Result<BTreeMap<String, Vec<String>>, String> {
    let data = payload(raw)?;
    let mut r = Reader {
        data: &data,
        pos: 0,
    };
    r.skip_section()?;
    r.skip_section()?;
    let section = r.u32()? as usize;
    if section < 9 {
        return Err("Некорректный список файлов".into());
    }
    let mut r = Reader {
        data: r.take(section - 4)?,
        pos: 0,
    };
    r.take(1)?;
    let count = r.u32()? as usize;
    if count > 1_000_000 {
        return Err("Слишком много файлов".into());
    }
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        names.push(r.string()?);
    }
    for _ in 0..count {
        if !r.string()?.is_empty() {
            return Err("Ссылки в установке движка не поддерживаются".into());
        }
    }
    r.take(count * 20)?;
    r.take(count)?;
    let mut result = BTreeMap::new();
    for name in names {
        let n = r.u32()? as usize;
        if n > 128 {
            return Err("Некорректные компоненты".into());
        }
        let mut tags = Vec::new();
        for _ in 0..n {
            tags.push(r.string()?);
        }
        if result.insert(name, tags).is_some() {
            return Err("Повтор файла в манифесте".into());
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truncated_rejected() {
        for n in 0..80 {
            assert!(tags(&vec![0; n]).is_err());
        }
    }
}
