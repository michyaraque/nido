use super::*;

mod bills;
mod expenses;
mod home;
mod house;
mod settings;
mod shopping;
mod tasks;
mod ticket;
mod copies;

pub use ticket::ticket_page;
pub use copies::backups_page;

pub struct U {
    pub id: i64,
    pub name: String,
    pub salary: i64,
    pub photo: i64, // version of the profile photo, 0 = none
}
pub struct Bill {
    pub id: i64,
    pub name: String,
    pub amount: i64, // this month (one-off amount if set)
    pub base: i64,
    pub due_day: i64,
    pub owner: Option<i64>,      // Some = personal bill, only its owner sees it
    pub payer: Option<i64>,      // Some = paid this month
    pub covered_by: Option<i64>, // house bill that this person always pays in full
    pub whole: bool,             // this month's payer paid it in full
}
pub struct Exp {
    pub id: i64,
    pub description: String,
    pub amount: i64,
    pub source: String,
    pub category: Option<String>,
    pub date: String,
    pub weekday: usize,
    pub ticket: Option<i64>,
    pub note: Option<String>,
    pub user_id: i64,
}
pub struct Settle {
    pub id: i64,
    pub from: i64,
    pub to: i64,
    pub amount: i64,
}
pub struct Item {
    pub id: i64,
    pub name: String,
    pub done: bool,
    pub added_by: Option<i64>,
}
pub struct Chore {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub color: String,
    pub every_days: Option<i64>,
    pub last_user: Option<i64>,
    pub days_since: Option<i64>,
}
pub struct Log {
    pub id: i64,
    pub chore_id: i64,
    pub user_id: i64,
    pub day: String,
    pub name: String,
    pub icon: String,
    pub color: String,
}
pub struct Plan {
    pub id: i64,
    pub chore_id: i64,
    pub day: String,
    pub assignee: Option<i64>, // None = anyone can take it
    pub name: String,
    pub icon: String,
    pub color: String,
}
pub struct PendingTicket {
    pub id: i64,
    pub status: String,
    pub merchant: Option<String>,
    pub total: Option<i64>,
}

const TABS: [(&str, &str, &str); 6] = [
    ("home", "Resumen", "house"),
    ("house", "Casa", "users"),
    ("expenses", "Gastos", "receipt"),
    ("shopping", "Compra", "shopping-basket"),
    ("tasks", "Tareas", "calendar-check"),
    ("settings", "Ajustes", "sliders-horizontal"),
];

pub struct View {
    pub me: User,
    pub users: Vec<U>,
    pub bills: Vec<Bill>,
    pub exps: Vec<Exp>,
    pub settles: Vec<Settle>,
    pub shopping: Vec<Item>,
    pub suggestions: Vec<String>,
    pub tickets: Vec<PendingTicket>,
    pub tg: Option<notify::Panel>,
    pub backups: Option<backup::Panel>,
    pub chores: Vec<Chore>,
    pub logs: Vec<Log>,
    pub plans: Vec<Plan>,
    pub share_expenses: bool,
    pub who: String,
    pub shown: Vec<Exp>,
    pub day: Option<String>,
    pub first_weekday: i64,  // 0 = Monday, for the first day of month m
    pub m: String,
    pub t: &'static str,
    pub today: String,
    pub yesterday: String,
    pub ndays: i64,
    pub host: String,
}

impl View {
    fn load(c: &Connection, me: User, m: String, t: &'static str, host: String, day: Option<&str>, who: Option<&str>) -> rusqlite::Result<View> {
        let users = c
            .prepare("SELECT id,name,salary,photo FROM users ORDER BY id")?
            .query_map([], |r| Ok(U { id: r.get(0)?, name: r.get(1)?, salary: r.get(2)?, photo: r.get(3)? }))?
            .collect::<Result<_, _>>()?;
        let bills = c
            .prepare(
                "SELECT b.id,b.name,COALESCE(a.amount,b.amount),b.amount,b.due_day,b.owner_id,p.payer_id,b.covered_by,COALESCE(p.whole,0) FROM bills b
                 LEFT JOIN payments p ON p.bill_id=b.id AND p.month=?1
                 LEFT JOIN bill_amounts a ON a.bill_id=b.id AND a.month=?1
                 WHERE b.start_month<=?1 AND (b.end_month IS NULL OR ?1<b.end_month)
                   AND (b.owner_id IS NULL OR b.owner_id=?2)
                 ORDER BY b.due_day, b.name",
            )?
            .query_map(params![m, me.id], |r| {
                Ok(Bill {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    amount: r.get(2)?,
                    base: r.get(3)?,
                    due_day: r.get(4)?,
                    owner: r.get(5)?,
                    payer: r.get(6)?,
                    covered_by: r.get(7)?,
                    whole: r.get(8)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let expenses_of = |user: Option<i64>| -> rusqlite::Result<Vec<Exp>> {
            c.prepare(
                "SELECT e.id,e.description,e.amount,e.source,e.category,date(e.created),CAST(strftime('%w',e.created) AS INTEGER),t.id,e.note,e.user_id
                 FROM expenses e LEFT JOIN tickets t ON t.expense_id=e.id
                 WHERE (?1 IS NULL OR e.user_id=?1) AND e.month=?2 ORDER BY e.created DESC, e.id DESC",
            )?
            .query_map(params![user, m], |r| {
                Ok(Exp {
                    id: r.get(0)?,
                    description: r.get(1)?,
                    amount: r.get(2)?,
                    source: r.get(3)?,
                    category: r.get(4)?,
                    date: r.get(5)?,
                    weekday: r.get::<_, i64>(6)? as usize,
                    ticket: r.get(7)?,
                    note: r.get(8)?,
                    user_id: r.get(9)?,
                })
            })?
            .collect()
        };
        let exps = expenses_of(Some(me.id))?;
        let settles = c
            .prepare("SELECT id,from_id,to_id,amount FROM settlements WHERE month=?1 ORDER BY id")?
            .query_map([&m], |r| Ok(Settle { id: r.get(0)?, from: r.get(1)?, to: r.get(2)?, amount: r.get(3)? }))?
            .collect::<Result<_, _>>()?;
        let shopping = c
            .prepare("SELECT id,name,done,added_by FROM shopping WHERE cleared=0 ORDER BY done, id DESC")?
            .query_map([], |r| Ok(Item { id: r.get(0)?, name: r.get(1)?, done: r.get(2)?, added_by: r.get(3)? }))?
            .collect::<Result<_, _>>()?;
        let suggestions = c
            .prepare(
                "SELECT name FROM shopping WHERE cleared=1
                   AND lower(name) NOT IN (SELECT lower(name) FROM shopping WHERE cleared=0)
                 GROUP BY lower(name) ORDER BY count(*) DESC, max(id) DESC LIMIT 12",
            )?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let tickets = c
            .prepare("SELECT id,status,merchant,total FROM tickets WHERE user_id=?1 AND status!='done' ORDER BY id DESC LIMIT 10")?
            .query_map([me.id], |r| Ok(PendingTicket { id: r.get(0)?, status: r.get(1)?, merchant: r.get(2)?, total: r.get(3)? }))?
            .collect::<Result<_, _>>()?;
        let today = today(c)?;
        let chores = c
            .prepare(
                "SELECT c.id,c.name,c.icon,c.color,c.every_days,l.user_id,CAST(julianday(?1)-julianday(l.day) AS INTEGER)
                 FROM chores c
                 LEFT JOIN chore_logs l ON l.id=(SELECT id FROM chore_logs WHERE chore_id=c.id AND day<=?1 ORDER BY day DESC, id DESC LIMIT 1)
                 WHERE c.archived=0 ORDER BY c.id",
            )?
            .query_map([&today], |r| {
                Ok(Chore {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    icon: r.get(2)?,
                    color: r.get(3)?,
                    every_days: r.get(4)?,
                    last_user: r.get(5)?,
                    days_since: r.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let logs = c
            .prepare(
                "SELECT l.id,l.chore_id,l.user_id,l.day,c.name,c.icon,c.color FROM chore_logs l
                 JOIN chores c ON c.id=l.chore_id WHERE substr(l.day,1,7)=?1 ORDER BY l.day, l.id",
            )?
            .query_map([&m], |r| {
                Ok(Log { id: r.get(0)?, chore_id: r.get(1)?, user_id: r.get(2)?, day: r.get(3)?, name: r.get(4)?, icon: r.get(5)?, color: r.get(6)? })
            })?
            .collect::<Result<_, _>>()?;
        let day = day
            .filter(|d| d.len() == 10 && d.starts_with(m.as_str()) && d[8..].parse::<i64>().is_ok())
            .map(str::to_string)
            .or_else(|| today.starts_with(m.as_str()).then(|| today.clone()));
        let plans = c
            .prepare(
                "SELECT p.id,p.chore_id,p.day,p.assignee_id,c.name,c.icon,c.color FROM chore_plans p
                 JOIN chores c ON c.id=p.chore_id
                 WHERE p.log_id IS NULL AND (substr(p.day,1,7)=?1 OR p.day<=date(?2,'+7 day'))
                 ORDER BY p.day, p.id",
            )?
            .query_map(params![m, today], |r| {
                Ok(Plan { id: r.get(0)?, chore_id: r.get(1)?, day: r.get(2)?, assignee: r.get(3)?, name: r.get(4)?, icon: r.get(5)?, color: r.get(6)? })
            })?
            .collect::<Result<_, _>>()?;
        let share_expenses = c
            .query_row("SELECT value='1' FROM settings WHERE key='share_expenses'", [], |r| r.get(0))
            .optional()?
            .unwrap_or(false);
        let who = match who {
            Some("all") if share_expenses => "all".to_string(),
            Some(id) if share_expenses && id.parse::<i64>().is_ok() => id.to_string(),
            _ => "me".to_string(),
        };
        let shown = match who.as_str() {
            "me" => expenses_of(Some(me.id))?,
            "all" => expenses_of(None)?,
            id => expenses_of(id.parse().ok())?,
        };
        let first_weekday: i64 = c.query_row("SELECT (CAST(strftime('%w', ?1 || '-01') AS INTEGER) + 6) % 7", [&m], |r| r.get(0))?;
        let tg = if t == "settings" || t == "household" { Some(notify::panel(c, me.id)?) } else { None };
        let backups = if t == "household" { Some(backup::panel(c)?) } else { None };
        Ok(View {
            tg,
            backups,
            today,
            chores,
            logs,
            plans,
            share_expenses,
            who,
            shown,
            day,
            first_weekday,
            yesterday: c.query_row("SELECT date('now','localtime','-1 day')", [], |r| r.get(0))?,
            ndays: days_in(c, &m)?,
            me,
            users,
            bills,
            exps,
            settles,
            shopping,
            suggestions,
            tickets,
            m,
            t,
            host,
        })
    }

    fn is_now(&self) -> bool {
        self.m == self.today[..7]
    }
    fn is_past(&self) -> bool {
        self.m.as_str() < &self.today[..7]
    }
    fn today_day(&self) -> i64 {
        self.today[8..].parse().unwrap_or(1)
    }
    fn days_left(&self) -> i64 {
        if self.is_now() { self.ndays - self.today_day() + 1 } else { 0 }
    }
    fn month_name(&self) -> &'static str {
        MONTHS[self.m[5..].parse::<usize>().unwrap() - 1]
    }
    fn month_lc(&self) -> String {
        self.month_name().to_lowercase()
    }
    fn year(&self) -> &str {
        &self.m[..4]
    }

    /// Shared bills are split proportionally to salary (evenly if nobody set one).
    fn part(&self, amount: i64, sal: i64) -> i64 {
        let total: i64 = self.users.iter().map(|u| u.salary).sum();
        if total > 0 { amount * sal / total } else { amount / self.users.len().max(1) as i64 }
    }
    fn share_pct(&self, sal: i64) -> f64 {
        let total: i64 = self.users.iter().map(|u| u.salary).sum();
        if total > 0 { sal as f64 * 100.0 / total as f64 } else { 100.0 / self.users.len().max(1) as f64 }
    }
    fn shared(&self) -> i64 {
        self.bills.iter().filter(|b| b.owner.is_none()).map(|b| b.amount).sum()
    }
    fn personal(&self) -> i64 {
        self.bills.iter().filter(|b| b.owner.is_some()).map(|b| b.amount).sum()
    }
    fn spent(&self) -> i64 {
        self.exps.iter().map(|e| e.amount).sum()
    }
    fn carrier(&self, b: &Bill) -> Option<i64> {
        if b.whole { b.payer } else { b.covered_by }
    }
    fn share_of(&self, b: &Bill, user: &U) -> i64 {
        match (b.owner, self.carrier(b)) {
            (Some(owner), _) => if owner == user.id { b.amount } else { 0 },
            (None, Some(c)) => if c == user.id { b.amount } else { 0 },
            (None, None) => self.part(b.amount, user.salary),
        }
    }
    fn house_share(&self, user: &U) -> i64 {
        self.bills.iter().filter(|b| b.owner.is_none()).map(|b| self.share_of(b, user)).sum()
    }
    fn me_u(&self) -> U {
        self.users.iter().find(|u| u.id == self.me.id).map(|u| U { id: u.id, name: u.name.clone(), salary: u.salary, photo: u.photo })
            .unwrap_or(U { id: self.me.id, name: self.me.name.clone(), salary: self.me.salary, photo: 0 })
    }
    fn my_house(&self) -> i64 {
        self.house_share(&self.me_u())
    }
    fn mode(b: Option<&Bill>) -> String {
        match b {
            Some(b) if b.owner.is_some() => "personal".into(),
            Some(Bill { covered_by: Some(u), .. }) => format!("user:{u}"),
            _ => "split".into(),
        }
    }
    fn who_pays(&self, current: &str) -> Markup {
        html! {
            div.field {
                label { "¿Quién la paga?" }
                div.chips role="radiogroup" aria-label="Quién la paga" {
                    label.chip-radio { input type="radio" name="mode" value="split" checked[current == "split"]; span { "Repartida según sueldo" } }
                    @for u in &self.users {
                        @let value = format!("user:{}", u.id);
                        label.chip-radio { input type="radio" name="mode" value=(value) checked[current == value];
                            span { @if u.id == self.me.id { "Yo, entera" } @else { (u.name) ", entera" } } }
                    }
                    label.chip-radio { input type="radio" name="mode" value="personal" checked[current == "personal"]; span { "Solo mía" } }
                }
                p.hint { "«Entera»: es de la casa y la ve todo el mundo, pero no se reparte ni genera deuda." }
            }
        }
    }
    fn left(&self) -> i64 {
        self.me.salary - self.my_house() - self.personal() - self.spent()
    }
    fn available(&self) -> i64 {
        self.me.salary - self.my_house() - self.personal()
    }
    fn bills_total(&self) -> i64 {
        self.bills.iter().map(|b| b.amount).sum()
    }
    fn pending(&self) -> i64 {
        self.bills.iter().filter(|b| b.payer.is_none()).map(|b| b.amount).sum()
    }
    fn net(&self) -> i64 {
        // Only bills that are split create debts; those paid in full by someone do not.
        let split = |b: &&Bill| b.owner.is_none() && self.carrier(b).is_none();
        let paid_shared: i64 = self.bills.iter().filter(split).filter(|b| b.payer.is_some()).map(|b| b.amount).sum();
        let paid_by_me: i64 = self.bills.iter().filter(split).filter(|b| b.payer == Some(self.me.id)).map(|b| b.amount).sum();
        let received: i64 = self.settles.iter().filter(|s| s.to == self.me.id).map(|s| s.amount).sum();
        let sent: i64 = self.settles.iter().filter(|s| s.from == self.me.id).map(|s| s.amount).sum();
        paid_by_me - self.part(paid_shared, self.me.salary) - received + sent
    }
    fn pct_of_salary(&self, v: i64) -> i64 {
        if self.me.salary > 0 { v * 100 / self.me.salary } else { 0 }
    }

    fn other(&self) -> Option<&U> {
        self.users.iter().find(|u| u.id != self.me.id)
    }
    fn name_of(&self, id: i64) -> &str {
        self.users.iter().find(|u| u.id == id).map(|u| u.name.as_str()).unwrap_or("?")
    }
    fn link(&self, tab: &str) -> String {
        format!("/?month={}&tab={tab}", self.m)
    }
    fn ctx(&self) -> Markup {
        html! { input type="hidden" name="month" value=(self.m); input type="hidden" name="tab" value=(self.t); }
    }
    fn avatar(&self, id: i64) -> Markup {
        let i = self.users.iter().position(|u| u.id == id).unwrap_or(0);
        let Some(u) = self.users.get(i) else { return html! {} };
        html! {
            @if u.photo > 0 {
                img.avatar src={ "/avatars/" (u.id) "?v=" (u.photo) } alt="" title=(u.name) width="30" height="30";
            } @else {
                span class={ "avatar p" (i % 4) } title=(u.name) { (initial(&u.name)) }
            }
        }
    }

    fn avatars(&self) -> Markup {
        html! { div.avatars { @for u in &self.users { (self.avatar(u.id)) } } }
    }

    fn head(&self, title: &str, sub: Markup) -> Markup {
        html! {
            div.page-head {
                div { h1 { (title) } p { (sub) } }
                @if self.t != "shopping" && self.t != "settings" { (self.month_nav()) }
            }
        }
    }

    fn month_nav(&self) -> Markup {
        let (m, t) = (&self.m, self.t);
        html! {
            nav.month aria-label="Cambiar de mes" {
                a.icon-btn href={ "/?month=" (shift(m, -1)) "&tab=" (t) } aria-label="Mes anterior" { (icon("chevron-left")) }
                span.month-label { (icon("calendar")) (self.month_name()) " " (self.year()) }
                a.icon-btn href={ "/?month=" (shift(m, 1)) "&tab=" (t) } aria-label="Mes siguiente" { (icon("chevron-right")) }
                @if !self.is_now() { a.btn.ghost.sm href={ "/?tab=" (t) } { "Volver a hoy" } }
            }
        }
    }

    fn shell(&self, content: Markup, flash_msg: Option<&str>) -> Markup {
        html! {
            a.skip href="#main" { "Saltar al contenido" }
            div.app {
                aside.side {
                    div.brand { (brand()) }
                    nav.side-nav aria-label="Secciones" {
                        @for (id, label, ic) in TABS {
                            @let on = id == self.t || (id == "settings" && self.t == "household");
                            a.on[on] href=(self.link(id)) aria-current=[on.then_some("page")] { (icon(ic)) (label) }
                        }
                    }
                    div.side-user {
                        (self.avatars())
                        span.side-name { (self.me.name) }
                        form.inline method="post" action="/logout" { button.icon-btn aria-label="Cerrar sesión" title="Cerrar sesión" { (icon("log-out")) } }
                    }
                }
                main #main.content {
                    header.mtop {
                        div.brand { (brand()) }
                        a.mtop-me href=(self.link("settings")) aria-label="Ajustes" { (self.avatars()) span.mtop-gear { (icon("settings")) } }
                    }
                    (content)
                }
            }
            nav.tabbar aria-label="Secciones" {
                @for &(id, label, ic) in TABS.iter().filter(|(id, ..)| *id != "settings") {
                    a.on[id == self.t] href=(self.link(id)) aria-current=[(id == self.t).then_some("page")] { (icon(ic)) span { (label) } }
                }
            }
            @if self.t == "shopping" {
                a.fab href="#add-item" aria-label="Añadir a la lista" { (icon("plus")) }
            } @else if self.t == "tasks" {
                a.fab href="#new-chore" aria-label="Nueva tarea" { (icon("plus")) }
            } @else {
                a.fab href={ (self.link("expenses")) "#new" } aria-label="Registrar gasto" { (icon("plus")) }
            }
            @if let Some(msg) = flash_msg { div.toast role="status" aria-live="polite" { (icon("circle-check")) (msg) } }
        }
    }
}

#[derive(Deserialize)]
pub struct DashQ {
    month: Option<String>,
    tab: Option<String>,
    ok: Option<String>,
    day: Option<String>,
    who: Option<String>,
}

pub async fn dashboard(State(db): State<Db>, h: HeaderMap, Query(q): Query<DashQ>) -> R {
    let c = lock(&db);
    let me = require_user!(c, h);
    let m = month(&c, q.month.as_deref())?;
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("nido.local").to_string();
    let v = View::load(&c, me, m, tab(q.tab.as_deref()), host, q.day.as_deref(), q.who.as_deref())?;
    drop(c);

    let content = match v.t {
        "house" => house::render(&v),
        "expenses" => expenses::render(&v),
        "settings" => settings::render(&v),
        "household" => settings::render_house(&v),
        "shopping" => shopping::render(&v),
        "tasks" => tasks::render(&v),
        _ => home::render(&v),
    };
    let body = v.shell(content, q.ok.as_deref().and_then(flash));
    Ok(page(&format!("{} {} · Nido", v.month_name(), v.year()), "", body))
}
