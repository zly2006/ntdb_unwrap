use ntdb_unwrap::{
    db::{self, OFFSET_VFS_NAME, model::Model, register_offset_vfs, try_decrypt_db},
    ntqq::DBDecryptInfo,
};
use rusqlite::{Connection, fallible_streaming_iterator::FallibleStreamingIterator};

fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    if args.len() < 3 {
        eprintln!("Usage: {} <dbfile> [pkey]", args[0]);
        std::process::exit(1);
    }
    let mut iter = args.into_iter();
    iter.next();
    let dbfile = iter.next().unwrap();
    let key = iter.next().unwrap();

    register_offset_vfs().expect("Failed to register offset_vfs");
    let conn = Connection::open(format!("file:{}?vfs={}", dbfile, OFFSET_VFS_NAME))
        .expect("Failed to open db");

    try_decrypt_db(
        &conn,
        DBDecryptInfo {
            key,
            // set to None to automatically guess
            cipher_hmac_algorithm: None,
        },
    )
    .expect("Failed to decrypt db");

    let count = conn
        .query_row("SELECT COUNT(*) FROM group_msg_table;", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("Failed to get count");
    eprintln!("Total rows: {}", count);
    // Open output database
    let output_db = Connection::open("output.db")
        .expect("Failed to create output database");

    // Create table in output database
    output_db.execute(
        "CREATE TABLE IF NOT EXISTS group_msg_table (
            id INTEGER PRIMARY KEY,
            msg_random INTEGER,
            seq_id INTEGER,
            chat_type INTEGER,
            msg_type INTEGER,
            sub_msg_type INTEGER,
            send_type INTEGER,
            sender_uid TEXT,
            peer_uid TEXT,
            peer_uin INTEGER,
            send_status INTEGER,
            send_time INTEGER,
            sender_group_name TEXT,
            sender_nickname TEXT,
            message BLOB,
            send_date INTEGER,
            at_flag INTEGER,
            reply_msg_seq INTEGER,
            group_number INTEGER,
            sender_uin INTEGER
        )",
        [],
    )
    .expect("Failed to create table");

    let mut stmt = conn
        .prepare("SELECT * FROM group_msg_table ORDER BY `40050` DESC ;")
        .expect("prepare stmt failed");

    let mut counter = 0;
    stmt.query([])
        .unwrap()
        .for_each(|row| {
            let m = db::model::GroupMsgTable::parse_row(row);
            if let Err(e) = m {
                eprintln!("Failed to parse row {}: {}", counter, e);
                return;
            }
            let m = m.unwrap();

            // Insert into output database
            output_db.execute(
                "INSERT INTO group_msg_table (id, msg_random, seq_id, chat_type, msg_type, sub_msg_type,
                    send_type, sender_uid, peer_uid, peer_uin, send_status, send_time, sender_group_name,
                    sender_nickname, message, send_date, at_flag, reply_msg_seq, group_number, sender_uin)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    m.id, m.msg_random, m.seq_id, m.chat_type.0, m.msg_type.0, m.sub_msg_type.0,
                    m.send_type, m.sender_uid, m.peer_uid, m.peer_uin, m.send_status.0, m.send_time,
                    m.sender_group_name, m.sender_nickname, m.message.as_ref().map(|msg| serde_json::to_vec(msg).unwrap()),
                    m.send_date, m.at_flag.0, m.reply_msg_seq, m.group_number, m.sender_uin
                ],
            )
            .expect("Failed to insert row");

            if counter % 100 == 0 {
                eprintln!("Processed {}/{} rows...", counter, count);
            }
            counter += 1;
        })
        .expect("Failed to query");

    eprintln!("Successfully exported {} rows to output.db", counter);
}
