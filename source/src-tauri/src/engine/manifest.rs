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
/// Read only file names for legacy library classification, with tighter limits
/// than an engine installation. Chunk tables and file payloads are never parsed.
fn json_package_filenames(raw:&[u8])->Result<Vec<String>,String>{
    #[derive(serde::Deserialize)]
    #[serde(rename_all="PascalCase")]
    struct File {filename:String,#[serde(default)] symlink_target:String}
    fn names<'de,D:serde::Deserializer<'de>>(de:D)->Result<Vec<String>,D::Error>{
        struct Names;
        impl<'de> serde::de::Visitor<'de> for Names {
            type Value=Vec<String>;
            fn expecting(&self,f:&mut std::fmt::Formatter)->std::fmt::Result{f.write_str("a bounded file manifest list")}
            fn visit_seq<A:serde::de::SeqAccess<'de>>(self,mut seq:A)->Result<Self::Value,A::Error>{
                use serde::de::Error;
                let mut result=Vec::new();let mut bytes=0usize;
                while let Some(file)=seq.next_element::<File>()?{
                    if result.len()>=250_000 || file.filename.is_empty() || file.filename.len()>2000 || file.filename.chars().any(char::is_control) || !file.symlink_target.is_empty(){return Err(A::Error::custom("Invalid package file record"));}
                    crate::vault::relative(&file.filename).map_err(|_|A::Error::custom("Unsafe package path"))?;
                    bytes=bytes.checked_add(file.filename.len()).ok_or_else(||A::Error::custom("Package name bound exceeded"))?;
                    if bytes>32*1024*1024{return Err(A::Error::custom("Package name bound exceeded"));}
                    result.push(file.filename);
                }
                if result.is_empty(){return Err(A::Error::custom("Empty package file list"));}
                Ok(result)
            }
        }
        de.deserialize_seq(Names)
    }
    #[derive(serde::Deserialize)]
    struct Manifest {#[serde(rename="FileManifestList",deserialize_with="names")] files:Vec<String>}
    serde_json::from_slice::<Manifest>(raw).map(|value|value.files).map_err(|_|"Некорректное JSON-описание пакета".into())
}

pub(super) fn package_filenames(raw:&[u8])->Result<Vec<String>,String>{
    if raw.len()>8*1024*1024 {return Err("Описание пакета слишком велико".into());}
    let raw=raw.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(raw);
    if raw.iter().find(|byte|!byte.is_ascii_whitespace())==Some(&b'{'){return json_package_filenames(raw);}
    if raw.len()<41 {return Err("Описание пакета обрезано".into());}
    let expanded=u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    if expanded>64*1024*1024 {return Err("Описание пакета слишком велико".into());}
    let data=payload(raw)?;
    let mut all=Reader{data:&data,pos:0};
    all.skip_section()?;all.skip_section()?;
    let size=all.u32()? as usize;
    if size<9 {return Err("Некорректный список файлов пакета".into());}
    let mut files=Reader{data:all.take(size-4)?,pos:0};files.take(1)?;
    let count=files.u32()? as usize;
    if count==0 || count>250_000 || count>files.data.len()/4 {return Err("Некорректное число файлов пакета".into());}
    let mut names=Vec::with_capacity(count);let mut bytes=0usize;
    for _ in 0..count {
        let name=files.string()?;
        if name.is_empty() || name.len()>2000 || name.chars().any(char::is_control) {return Err("Некорректное имя файла пакета".into());}
        crate::vault::relative(&name)?;
        bytes=bytes.checked_add(name.len()).ok_or("Слишком много имён файлов")?;
        if bytes>32*1024*1024 {return Err("Слишком много имён файлов".into());}
        names.push(name);
    }
    for _ in 0..count {if !files.string()?.is_empty(){return Err("Ссылки в пакете не поддерживаются".into());}}
    files.take(count.checked_mul(21).ok_or("Некорректное число файлов")?)?;
    Ok(names)
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
    fn package_fixture(names:&[&str],compressed:bool)->Vec<u8>{
        use std::io::Write;
        fn string(out:&mut Vec<u8>,value:&str){out.extend_from_slice(&((value.len()+1) as u32).to_le_bytes());out.extend_from_slice(value.as_bytes());out.push(0);}
        let mut files=vec![0];files.extend_from_slice(&(names.len()as u32).to_le_bytes());
        for name in names {string(&mut files,name);}for _ in names {files.extend_from_slice(&0u32.to_le_bytes());}
        files.extend(std::iter::repeat_n(0,names.len()*21));
        let mut data=Vec::new();for _ in 0..2 {data.extend_from_slice(&5u32.to_le_bytes());data.push(0);}
        data.extend_from_slice(&((files.len()+4)as u32).to_le_bytes());data.extend(files);
        let packed=if compressed {let mut zip=flate2::write::ZlibEncoder::new(Vec::new(),flate2::Compression::default());zip.write_all(&data).unwrap();zip.finish().unwrap()}else{data.clone()};
        let mut raw=Vec::new();for value in [1153351692u32,41,data.len()as u32,packed.len()as u32]{raw.extend_from_slice(&value.to_le_bytes());}
        raw.extend_from_slice(&Sha1::digest(&data));raw.push(u8::from(compressed));raw.extend_from_slice(&0u32.to_le_bytes());raw.extend(packed);raw
    }
    #[test]fn legacy_package_names_are_read_without_chunk_tables(){
        let names=["Hillside.uproject","Config/DefaultEngine.ini","Content/Map.umap"];
        for compressed in [false,true]{let got=package_filenames(&package_fixture(&names,compressed)).unwrap();assert_eq!(got,names);assert_eq!(crate::content_kind::from_paths("Asset",&got),"Project");}
    }
    #[test]fn legacy_package_names_reject_tamper_bombs_and_bad_paths(){
        let mut raw=package_fixture(&["Game.uproject","Content/Map.umap"],true);raw[20]^=1;assert!(package_filenames(&raw).is_err());
        let mut raw=package_fixture(&["Game.uproject"],true);raw[8..12].copy_from_slice(&(64u32*1024*1024+1).to_le_bytes());assert!(package_filenames(&raw).is_err());
        for name in ["../Game.uproject","C:/Game.uproject",""]{assert!(package_filenames(&package_fixture(&[name],false)).is_err());}
        let raw=package_fixture(&["Game.uproject"],false);for cut in 0..raw.len(){assert!(package_filenames(&raw[..cut]).is_err());}
    }

    #[test]fn legacy_json_reads_only_bounded_file_names(){
        let raw=br#"{"FileManifestList":[{"Filename":"Legacy.uproject","FileChunkParts":[{"ignored":[1,2,3]}]},{"Filename":"Config/DefaultGame.ini"},{"Filename":"Content/Map.umap"}],"ChunkHashList":{"ignored":"metadata"}}"#;
        let names=package_filenames(raw).unwrap();assert_eq!(names.len(),3);assert_eq!(crate::content_kind::from_paths("Asset",&names),"Project");
        let mut bom=vec![0xef,0xbb,0xbf];bom.extend_from_slice(raw);assert_eq!(package_filenames(&bom).unwrap(),names);
    }
    #[test]fn legacy_json_rejects_missing_empty_duplicate_and_unsafe_file_lists(){
        for raw in [br#"{}"#.as_slice(),br#"{"FileManifestList":[]}"#,br#"{"FileManifestList":[{"Filename":"../Game.uproject"}]}"#,br#"{"FileManifestList":[{"Filename":"Game.uproject","SymlinkTarget":"outside"}]}"#,br#"{"FileManifestList":[{"Filename":"Game.uproject"}],"FileManifestList":[]}"#]{assert!(package_filenames(raw).is_err());}
    }

}
