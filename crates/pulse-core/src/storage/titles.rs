use super::{FileCursor, Store, StoreError};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::params;
use serde::Deserialize;

pub struct SessionTitle {
    id: String,
    thread_name: String,
    updated_at: String,
}
impl SessionTitle {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        #[derive(Deserialize)]
        struct Record {
            id: String,
            thread_name: String,
            updated_at: String,
        }
        let row: Record = serde_json::from_slice(bytes).map_err(|_| "invalid_title_record")?;
        if row.id.is_empty() || row.id.len() > 256 {
            return Err("invalid_title_id");
        }
        let updated_at = DateTime::parse_from_rfc3339(&row.updated_at)
            .map_err(|_| "invalid_title_time")?
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Nanos, true);
        Ok(Self {
            id: row.id,
            thread_name: row.thread_name.trim().chars().take(512).collect(),
            updated_at,
        })
    }
}
impl Store {
    pub fn commit_titles(
        &mut self,
        cursor: &FileCursor,
        titles: &[SessionTitle],
        issues: &[String],
    ) -> Result<usize, StoreError> {
        let offset =
            i64::try_from(cursor.offset).map_err(|_| crate::domain::DataError::Overflow)?;
        let tx = self.connection.transaction()?;
        let mut changed = 0;
        for title in titles {
            let name = (!title.thread_name.is_empty()).then_some(title.thread_name.as_str());
            // Newer titles win across mirrors; Windows owns conflicting equal timestamps.
            changed += tx.execute(
                "INSERT INTO session_titles VALUES(?1,?2,?3,?4)
                 ON CONFLICT(session_id) DO UPDATE SET title=excluded.title,
                   updated_at=excluded.updated_at,source_id=excluded.source_id
                 WHERE excluded.updated_at>session_titles.updated_at OR
                   (excluded.updated_at=session_titles.updated_at AND
                     ((excluded.source_id=session_titles.source_id AND excluded.title IS NOT session_titles.title)
                       OR (excluded.source_id='windows' AND session_titles.source_id!='windows')))",
                params![title.id,name,title.updated_at,cursor.source_id],
            )?;
        }
        for code in issues {
            tx.execute("INSERT INTO parse_issues(source_id,file_key,code) VALUES(?1,?2,?3) ON CONFLICT(source_id,file_key,code) DO UPDATE SET count=count+1",params![cursor.source_id,cursor.file_key,code])?;
        }
        tx.execute("INSERT INTO file_cursors VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(source_id,file_key) DO UPDATE SET offset=excluded.offset,file_identity=excluded.file_identity,modified_ns=excluded.modified_ns,parser_version=excluded.parser_version,parser_state=excluded.parser_state",params![cursor.source_id,cursor.file_key,offset,cursor.file_identity,cursor.modified_ns,crate::PARSER_VERSION,serde_json::to_string(&cursor.state)?])?;
        tx.commit()?;
        Ok(changed)
    }
}
