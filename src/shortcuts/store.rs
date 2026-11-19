//! Tags, their actions and their scans in the core database.
use super::*;

#[derive(Clone, Debug)]
pub struct Step {
    pub pos: i64,
    pub source: String,
    pub action: String,
    pub arg: String,
    pub label: String,
    pub icon: String,
    pub confirm: bool,
}

pub struct Tag {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub choose: bool,  // the scan offers a menu instead of doing everything
    pub confirm: bool, // ask before doing anything
    pub steps: Vec<Step>,
    pub last: Option<(String, i64)>, // last scan: when and who
}

impl Tag {
    pub fn asks(&self) -> bool {
        self.confirm || self.steps.iter().any(|s| s.confirm)
    }
    /// Only places to go: the scan goes straight there.
    pub fn just_opens(&self) -> bool {
        !self.choose && self.steps.len() == 1 && self.steps[0].source == "core" && self.steps[0].action.starts_with("open:")
    }
}

/// One line of a scan's result, as the page shows it.
#[derive(Serialize, Deserialize, Clone)]
pub struct Line {
    pub pos: i64,
    pub label: String,
    pub icon: String,
    pub status: String,
    pub text: String,
    pub href: Option<String>,
}

pub struct Run {
    pub id: i64,
    pub tag_id: i64,
    pub user_id: i64,
    pub at: String,
    pub lines: Vec<Line>,
    pub undo: Vec<(String, String)>,
    pub undone: bool,
    pub minutes: i64, // how long ago
}

impl Run {
    pub fn can_undo(&self, me: i64) -> bool {
        self.user_id == me && !self.undone && !self.undo.is_empty() && self.minutes < UNDO_MINUTES
    }
}

/// 64 random bits: short enough for the smallest tags (NTAG213 holds 137 bytes of URL).
pub fn new_code() -> String {
    rand_hex(8)
}

pub fn valid_code(s: &str) -> bool {
    (8..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn steps_of(c: &Connection, tag: Option<i64>) -> rusqlite::Result<Vec<(i64, Step)>> {
    let row = |r: &rusqlite::Row| {
        Ok((r.get(0)?, Step { pos: r.get(1)?, source: r.get(2)?, action: r.get(3)?, arg: r.get(4)?, label: r.get(5)?, icon: r.get(6)?, confirm: r.get(7)? }))
    };
    const SEL: &str = "SELECT tag_id,pos,source,action,arg,label,icon,confirm FROM tag_actions";
    match tag {
        Some(t) => c.prepare_cached(&format!("{SEL} WHERE tag_id=?1 ORDER BY pos"))?.query_map([t], row)?.collect(),
        None => c.prepare_cached(&format!("{SEL} ORDER BY tag_id,pos"))?.query_map([], row)?.collect(),
    }
}

fn one(c: &Connection, by: &str, value: &dyn rusqlite::ToSql) -> rusqlite::Result<Option<Tag>> {
    let tag = c
        .query_row(&format!("SELECT id,code,name,mode='choose',confirm FROM tags WHERE {by}=?1"), [value], |r| {
            Ok(Tag { id: r.get(0)?, code: r.get(1)?, name: r.get(2)?, choose: r.get(3)?, confirm: r.get(4)?, steps: Vec::new(), last: None })
        })
        .optional()?;
    let Some(mut tag) = tag else { return Ok(None) };
    tag.steps = steps_of(c, Some(tag.id))?.into_iter().map(|(_, s)| s).collect();
    Ok(Some(tag))
}

pub fn by_code(c: &Connection, code: &str) -> rusqlite::Result<Option<Tag>> {
    one(c, "code", &code)
}

pub fn by_id(c: &Connection, id: i64) -> rusqlite::Result<Option<Tag>> {
    one(c, "id", &id)
}

/// Every tag with its actions and its last scan, newest first. Three queries, whatever the count.
pub fn all(c: &Connection) -> rusqlite::Result<Vec<Tag>> {
    let mut tags: Vec<Tag> = c
        .prepare("SELECT id,code,name,mode='choose',confirm FROM tags ORDER BY id DESC")?
        .query_map([], |r| Ok(Tag { id: r.get(0)?, code: r.get(1)?, name: r.get(2)?, choose: r.get(3)?, confirm: r.get(4)?, steps: Vec::new(), last: None }))?
        .collect::<Result<_, _>>()?;
    for (tag, step) in steps_of(c, None)? {
        if let Some(t) = tags.iter_mut().find(|t| t.id == tag) {
            t.steps.push(step);
        }
    }
    // SQLite takes the bare `user_id` from the row holding max(at).
    let last: Vec<(i64, String, i64)> = c
        .prepare("SELECT tag_id,max(at),user_id FROM tag_runs WHERE undone=0 GROUP BY tag_id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (tag, at, user) in last {
        if let Some(t) = tags.iter_mut().find(|t| t.id == tag) {
            t.last = Some((at, user));
        }
    }
    Ok(tags)
}

fn put_steps(c: &Connection, tag: i64, steps: &[Step]) -> rusqlite::Result<()> {
    c.execute("DELETE FROM tag_actions WHERE tag_id=?1", [tag])?;
    for (i, s) in steps.iter().enumerate() {
        c.execute(
            "INSERT INTO tag_actions(tag_id,pos,source,action,arg,label,icon,confirm) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![tag, i as i64, s.source, s.action, s.arg, s.label, s.icon, s.confirm],
        )?;
    }
    Ok(())
}

pub fn create(c: &Connection, code: &str, name: &str, choose: bool, confirm: bool, steps: &[Step], by: i64) -> rusqlite::Result<i64> {
    let tx = c.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO tags(code,name,mode,confirm,created_by) VALUES(?1,?2,?3,?4,?5)",
        params![code, name, if choose { "choose" } else { "all" }, confirm, by],
    )?;
    let id = tx.last_insert_rowid();
    put_steps(&tx, id, steps)?;
    tx.commit()?;
    Ok(id)
}

/// Same id and same code: the tag on the wall keeps working.
pub fn update(c: &Connection, id: i64, name: &str, choose: bool, confirm: bool, steps: &[Step]) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute("UPDATE tags SET name=?2,mode=?3,confirm=?4 WHERE id=?1", params![id, name, if choose { "choose" } else { "all" }, confirm])?;
    put_steps(&tx, id, steps)?;
    tx.commit()
}

pub fn delete(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM tags WHERE id=?1", [id])?;
    Ok(())
}

/// Keeps a scan. Scans older than a year only fed "last used", so they go.
pub fn record(c: &Connection, tag: i64, user: i64, lines: &[Line], undo: &[(String, String)]) -> rusqlite::Result<i64> {
    let lines = serde_json::to_string(lines).unwrap_or_else(|_| "[]".into());
    let undo = serde_json::to_string(undo).unwrap_or_else(|_| "[]".into());
    c.execute("INSERT INTO tag_runs(tag_id,user_id,result,undo) VALUES(?1,?2,?3,?4)", params![tag, user, lines, undo])?;
    let id = c.last_insert_rowid();
    c.execute("DELETE FROM tag_runs WHERE at<datetime('now','localtime','-365 days')", [])?;
    Ok(id)
}

pub fn run(c: &Connection, id: i64) -> rusqlite::Result<Option<Run>> {
    c.query_row(
        "SELECT id,tag_id,user_id,at,result,undo,undone,CAST((julianday('now','localtime')-julianday(at))*1440 AS INTEGER) FROM tag_runs WHERE id=?1",
        [id],
        |r| {
            Ok(Run {
                id: r.get(0)?,
                tag_id: r.get(1)?,
                user_id: r.get(2)?,
                at: r.get(3)?,
                lines: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                undo: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                undone: r.get(6)?,
                minutes: r.get(7)?,
            })
        },
    )
    .optional()
}

/// My last scan of this tag if it was a moment ago and still stands.
pub fn recent(c: &Connection, tag: i64, user: i64) -> rusqlite::Result<Option<i64>> {
    c.query_row(
        &format!("SELECT id FROM tag_runs WHERE tag_id=?1 AND at>=datetime('now','localtime','-{REPEAT_SECONDS} seconds') AND user_id=?2 AND undone=0 ORDER BY at DESC LIMIT 1"),
        params![tag, user],
        |r| r.get(0),
    )
    .optional()
}

pub fn mark_undone(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE tag_runs SET undone=1 WHERE id=?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(source: &str, action: &str) -> Step {
        Step { pos: 0, source: source.into(), action: action.into(), arg: String::new(), label: action.into(), icon: "zap".into(), confirm: false }
    }

    #[test]
    fn tags_keep_their_code_and_order() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c);
        c.execute("INSERT INTO users(id,name,pass,token) VALUES(1,'Ana','x','a'),(2,'Leo','x','b')", []).unwrap();
        let id = create(&c, "abcdef0123456789", "Bol", false, false, &[step("pets", "meal:3"), step("core", "shop")], 1).unwrap();
        update(&c, id, "Junto al bol", true, false, &[step("core", "shop"), step("pets", "walk:3"), step("pets", "meal:3")]).unwrap();
        let t = by_code(&c, "abcdef0123456789").unwrap().unwrap();
        assert_eq!((t.id, t.name.as_str(), t.choose), (id, "Junto al bol", true));
        assert_eq!(t.steps.iter().map(|s| (s.pos, s.action.as_str())).collect::<Vec<_>>(), [(0, "shop"), (1, "walk:3"), (2, "meal:3")]);

        let line = Line { pos: 0, label: "x".into(), icon: "zap".into(), status: "done".into(), text: String::new(), href: None };
        let r1 = record(&c, id, 1, std::slice::from_ref(&line), &[("core".into(), "s1".into())]).unwrap();
        assert_eq!(recent(&c, id, 1).unwrap(), Some(r1));
        assert_eq!(recent(&c, id, 2).unwrap(), None, "another person's scan is not mine");
        let r = run(&c, r1).unwrap().unwrap();
        assert!(r.can_undo(1) && !r.can_undo(2));
        mark_undone(&c, r1).unwrap();
        assert!(!run(&c, r1).unwrap().unwrap().can_undo(1));
        assert_eq!(recent(&c, id, 1).unwrap(), None);

        record(&c, id, 2, &[line], &[]).unwrap();
        let listed = all(&c).unwrap();
        assert_eq!(listed[0].last.as_ref().map(|l| l.1), Some(2));
        delete(&c, id).unwrap();
        assert!(by_id(&c, id).unwrap().is_none());
        assert_eq!(c.query_row("SELECT count(*) FROM tag_actions", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn codes() {
        assert!(valid_code(&new_code()) && valid_code("abc-DEF_123"));
        assert!(!valid_code("short") && !valid_code("abc/../../x") && !valid_code("abcdefgh?x=1"));
    }
}
