use super::*;
use crate::Result;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct C2CMessageTable {
    pub id: i64,
    pub msg_random: i64,
    pub seq_id: i64,
    pub chat_type: ChatType,
    pub msg_type: MessageType,
    pub sub_msg_type: SubMessageType,
    pub send_type: i64,
    pub sender_uid: String,
    pub send_status: SendStatus,
    pub send_time: i64,
    pub sender_nickname: String,
    pub message: Option<Message>,
    pub send_date: i64,
    pub at_flag: AtFlag,
    pub reply_msg_seq: i64,
    pub friend_uin: i64,
    pub sender_uin: i64,
}

impl Model for C2CMessageTable {
    fn parse_row(row: &rusqlite::Row) -> Result<Self> {
        Ok(Self {
            id: map_field!(row, "40001")?,
            msg_random: map_field!(row, "40002")?,
            seq_id: map_field!(row, "40005")?, // Note: different field number than GroupMsgTable
            chat_type: map_field!(row, "40010")?,
            msg_type: map_field!(row, "40011")?,
            sub_msg_type: map_field!(row, "40012")?,
            send_type: map_field!(row, "40013")?,
            sender_uid: map_field!(row, "40020")?,
            send_status: map_field!(row, "40041")?,
            send_time: map_field!(row, "40050")?,
            sender_nickname: map_field!(row, "40093")?,
            message: map_field!(row, "40800")?,
            send_date: map_field!(row, "40058")?,
            at_flag: map_field!(row, "40100")?,
            reply_msg_seq: map_field!(row, "40850")?,
            friend_uin: map_field!(row, "40030")?,
            sender_uin: map_field!(row, "40033")?,
        })
    }
}

