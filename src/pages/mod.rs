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
    pub color: String, // "#rrggbb"; defaults to PALETTE by position
    pub share: i64,    // % for the "custom" split
}

/// Income of month `m` for one person. A ticked income counts in the month it belongs to,
/// whenever it was ticked (marking July's payslip in November still fills July).
/// `tracked`: the person confirms their fixed income by hand, so it only counts when ticked.
/// Otherwise it counts as received every month.
pub fn cash_in(m: &str, salary: i64, tracked: bool, salaries: &[(String, String, i64)], received: &[(String, i64)]) -> i64 {
    let assumed = if salary > 0 && !tracked { salary } else { 0 };
    let ticked: i64 = salaries.iter().filter(|s| s.0 == m).map(|s| s.2).sum();
    let extras: i64 = received.iter().filter(|r| r.0 == m).map(|r| r.1).sum();
    assumed + ticked + extras
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

pub struct Income {
    pub id: i64,
    pub month: String,
    pub concept: String,
    pub amount: i64,
    pub received: Option<String>,
}

/// Something owed to me: label, amount, and the form that ticks it as received.
pub struct Owed {
    pub label: String,
    pub amount: i64,
    pub action: String,
}

pub struct View {
    pub me: User,
    pub incomes: Vec<Income>,
    pub salaries: Vec<(String, String, i64)>, // (month, day received, amount)
    pub pay_since: String,
    pub track_pay: bool,
    pub pay_day: Option<i64>,
    pub carry: bool,
    pub carry_since: String,
    pub carry_start: i64,
    pub first_month: String, // first month with anything recorded by me
    pub carried: i64, // what was left (or missing) in the months before m; set by `dashboard`
    pub users: Vec<U>,
    pub bills: Vec<Bill>,
    pub exps: Vec<Exp>,
    pub settles: Vec<Settle>,
    pub shopping: Vec<Item>,
    pub suggestions: Vec<String>,
    pub concepts: Vec<String>, // my most used expense descriptions, for the "Concepto" field
    pub tickets: Vec<PendingTicket>,
    pub tg: Option<notify::Panel>,
    pub backups: Option<backup::Panel>,
    pub chores: Vec<Chore>,
    pub logs: Vec<Log>,
    pub plans: Vec<Plan>,
    pub share_expenses: bool,
    pub theme: &'static str,
    pub split: &'static str,
    pub weights: Vec<(i64, i64)>, // (user id, weight) for shared bills
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
        let mut users: Vec<U> = c
            .prepare("SELECT id,name,salary,photo,color,share FROM users ORDER BY id")?
            .query_map([], |r| {
                Ok(U {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    salary: r.get(2)?,
                    photo: r.get(3)?,
                    color: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    share: r.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        for (i, u) in users.iter_mut().enumerate() {
            // Only palette colours reach the page (they go into an inline <style>).
            if !PALETTE.contains(&u.color.as_str()) {
                u.color = PALETTE[i % PALETTE.len()].to_string();
            }
        }
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
        let concepts = c
            .prepare(
                "SELECT trim(description) FROM expenses WHERE user_id=?1
                 GROUP BY lower(trim(description)) ORDER BY count(*) DESC, max(id) DESC LIMIT 80",
            )?
            .query_map([me.id], |r| r.get(0))?
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
        let incomes = c
            .prepare("SELECT id,month,concept,amount,received FROM incomes WHERE user_id=?1 ORDER BY month, id")?
            .query_map([me.id], |r| Ok(Income { id: r.get(0)?, month: r.get(1)?, concept: r.get(2)?, amount: r.get(3)?, received: r.get(4)? }))?
            .collect::<Result<_, _>>()?;
        let salaries = c
            .prepare("SELECT month,day,amount FROM salary_received WHERE user_id=?1")?
            .query_map([me.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        let (pay_since, track_pay, pay_day, carry, carry_since, carry_start): (String, bool, Option<i64>, bool, String, i64) = c.query_row(
            "SELECT COALESCE(pay_since, strftime('%Y-%m','now','localtime')), track_pay, pay_day, carry,
                    COALESCE(carry_since, strftime('%Y-%m','now','localtime')), carry_start FROM users WHERE id=?1",
            [me.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )?;
        let first_month: String = c.query_row(
            "SELECT COALESCE(MIN(m), strftime('%Y-%m','now','localtime')) FROM (
               SELECT month AS m FROM expenses WHERE user_id=?1
               UNION ALL SELECT month FROM incomes WHERE user_id=?1
               UNION ALL SELECT month FROM salary_received WHERE user_id=?1)",
            [me.id],
            |r| r.get(0),
        )?;
        let theme: Option<String> = c.query_row("SELECT value FROM settings WHERE key='theme'", [], |r| r.get(0)).optional()?;
        let theme = theme_id(theme.as_deref().unwrap_or(""));
        let split: Option<String> = c.query_row("SELECT value FROM settings WHERE key='split'", [], |r| r.get(0)).optional()?;
        let split = split_id(split.as_deref().unwrap_or(""));
        let weights = weights(c, split, &users, &m)?;
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
            incomes,
            salaries,
            pay_since,
            track_pay,
            pay_day,
            carry,
            carry_since,
            carry_start,
            first_month,
            carried: 0,
            tg,
            backups,
            today,
            chores,
            logs,
            plans,
            share_expenses,
            theme,
            split,
            weights,
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
            concepts,
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

    /// Shared bills are split by each person's weight (see `weights`); evenly if all weights are 0.
    fn part(&self, amount: i64, user: i64) -> i64 {
        let total: i64 = self.weights.iter().map(|w| w.1).sum();
        let mine = self.weights.iter().find(|w| w.0 == user).map_or(0, |w| w.1);
        if total > 0 { amount * mine / total } else { amount / self.users.len().max(1) as i64 }
    }
    fn share_pct(&self, user: i64) -> f64 {
        let total: i64 = self.weights.iter().map(|w| w.1).sum();
        let mine = self.weights.iter().find(|w| w.0 == user).map_or(0, |w| w.1);
        if total > 0 { mine as f64 * 100.0 / total as f64 } else { 100.0 / self.users.len().max(1) as f64 }
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
            (None, None) => self.part(b.amount, user.id),
        }
    }
    fn house_share(&self, user: &U) -> i64 {
        self.bills.iter().filter(|b| b.owner.is_none()).map(|b| self.share_of(b, user)).sum()
    }
    fn me_u(&self) -> U {
        self.users.iter().find(|u| u.id == self.me.id).map(|u| U { id: u.id, name: u.name.clone(), salary: u.salary, photo: u.photo, color: u.color.clone(), share: u.share })
            .unwrap_or(U { id: self.me.id, name: self.me.name.clone(), salary: self.me.salary, photo: 0, color: PALETTE[0].into(), share: 0 })
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
                    label.chip-radio { input type="radio" name="mode" value="split" checked[current == "split"]; span { "Repartida" } }
                    @for u in &self.users {
                        @let value = format!("user:{}", u.id);
                        label.chip-radio { input type="radio" name="mode" value=(value) checked[current == value];
                            span { @if u.id == self.me.id { "Yo, entera" } @else { (u.name) ", entera" } } }
                    }
                    label.chip-radio { input type="radio" name="mode" value="personal" checked[current == "personal"]; span { "Solo mía" } }
                }
                p.hint { "«Entera»: es de la casa y la ve todo el mundo, pero no se reparte ni genera deuda" }
            }
        }
    }
    /// Whether the fixed income of `m` must be ticked by hand (any month: nothing is assumed).
    /// Only months from `pay_since` on count as pending when unticked.
    fn tracks_salary(&self, _m: &str) -> bool {
        self.me.salary > 0 && self.track_pay
    }
    fn salary_day(&self, m: &str) -> Option<&str> {
        self.salaries.iter().find(|s| s.0 == m).map(|s| s.1.as_str())
    }
    /// Money that actually came in during month `m` (cash basis: a late payslip counts when it arrives).
    fn income_in(&self, m: &str) -> i64 {
        let received: Vec<(String, i64)> = self.incomes.iter().filter(|i| i.received.is_some()).map(|i| (i.month.clone(), i.amount)).collect();
        cash_in(m, self.me.salary, self.track_pay, &self.salaries, &received)
    }
    fn income(&self) -> i64 {
        self.income_in(&self.m)
    }
    /// What this month should bring: the payslip plus this month's one-off incomes.
    fn expected(&self) -> i64 {
        let salary = if self.me.salary > 0 { self.me.salary } else { 0 };
        salary + self.incomes.iter().filter(|i| i.month == self.m).map(|i| i.amount).sum::<i64>()
    }
    fn month_incomes(&self) -> impl Iterator<Item = &Income> {
        self.incomes.iter().filter(|i| i.month == self.m)
    }
    /// Pending from the months before the one on screen: unconfirmed fixed income (only if the
    /// person tracks it) and one-off incomes. Looking at July shows what was pending before July.
    fn owed(&self) -> Vec<Owed> {
        let now = self.m.as_str();
        let mut out = Vec::new();
        if self.me.salary > 0 && self.track_pay {
            let mut m = self.pay_since.clone();
            while m.as_str() < now {
                if self.salary_day(&m).is_none() {
                    let name = MONTHS[m[5..].parse::<usize>().unwrap_or(1) - 1].to_lowercase();
                    out.push(Owed { label: format!("Ingreso fijo de {name}"), amount: self.me.salary, action: format!("/incomes/salary/{m}") });
                }
                m = shift(&m, 1);
            }
        }
        for i in self.incomes.iter().filter(|i| i.received.is_none() && i.month.as_str() < now) {
            out.push(Owed { label: i.concept.clone(), amount: i.amount, action: format!("/incomes/{}/toggle", i.id) });
        }
        out
    }
    /// What I have: this month's leftover plus, if I carry it, what was left in earlier months.
    fn balance(&self) -> i64 {
        self.carried + self.left()
    }
    fn left(&self) -> i64 {
        self.income() - self.my_house() - self.personal() - self.spent()
    }
    fn available(&self) -> i64 {
        self.income() - self.my_house() - self.personal()
    }
    fn bills_total(&self) -> i64 {
        self.bills.iter().map(|b| b.amount).sum()
    }
    fn pending(&self) -> i64 {
        self.bills.iter().filter(|b| b.payer.is_none()).map(|b| b.amount).sum()
    }
    /// Each person's balance with the house this month: > 0 the others owe them.
    /// Only split bills create debts; those paid in full by someone do not.
    fn balances(&self) -> Vec<(i64, i64)> {
        let split = |b: &&Bill| b.owner.is_none() && self.carrier(b).is_none();
        let paid_shared: i64 = self.bills.iter().filter(split).filter(|b| b.payer.is_some()).map(|b| b.amount).sum();
        self.users
            .iter()
            .map(|u| {
                let paid: i64 = self.bills.iter().filter(split).filter(|b| b.payer == Some(u.id)).map(|b| b.amount).sum();
                let received: i64 = self.settles.iter().filter(|s| s.to == u.id).map(|s| s.amount).sum();
                let sent: i64 = self.settles.iter().filter(|s| s.from == u.id).map(|s| s.amount).sum();
                (u.id, paid - self.part(paid_shared, u.id) - received + sent)
            })
            .collect()
    }
    /// Fewest payments that settle everyone: (from, to, amount).
    fn transfers(&self) -> Vec<(i64, i64, i64)> {
        settle_up(self.balances())
    }
    fn pct_of_income(&self, v: i64) -> i64 {
        let income = self.income();
        if income > 0 { v * 100 / income } else { 0 }
    }

    fn name_of(&self, id: i64) -> &str {
        self.users.iter().find(|u| u.id == id).map(|u| u.name.as_str()).unwrap_or("?")
    }
    fn link(&self, tab: &str) -> String {
        format!("/?month={}&tab={tab}", self.m)
    }
    /// Past expense descriptions as ready-made options (logo, name, category). app.js filters them
    /// under the focused field marked `data-concepts="concepts"`.
    fn concept_list(&self) -> Markup {
        html! {
            template #concepts {
                @for c in &self.concepts {
                    @let l = look(c, None);
                    button.opt type="button" role="option" data-v=(c) {
                        (badge(&l, c))
                        span.opt-txt { (c) small { (l.cat.label) } }
                    }
                }
            }
        }
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
                span class={ "avatar p" (i) } title=(u.name) { (initial(&u.name)) }
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
                @if !matches!(self.t, "shopping" | "settings" | "household") { (self.month_nav()) }
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

    /// Count shown next to a sidebar entry: what still needs doing there.
    fn badge(&self, id: &str) -> (usize, &'static str) {
        match id {
            "home" => (self.bills.iter().filter(|b| b.payer.is_none()).count(), "warn"),
            "shopping" => (self.shopping.iter().filter(|i| !i.done).count(), ""),
            "tasks" => (tasks::due(self).len(), "bad"),
            _ => (0, ""),
        }
    }
    fn side_link(&self, id: &str, label: &str, ic: &str) -> Markup {
        let on = id == self.t || (id == "settings" && self.t == "household");
        let (n, tone) = self.badge(id);
        html! {
            a.on[on] href=(self.link(id)) aria-current=[on.then_some("page")] {
                (icon(ic)) span.side-txt { (label) }
                @if n > 0 { span class={ "count " (tone) } aria-label={ (n) " pendientes" } { (n) } }
            }
        }
    }

    /// Each person's colour, by position: `.p{i}` paints a background, `.u{i}` sets `--u`.
    fn people_css(&self) -> String {
        self.users.iter().enumerate().map(|(i, u)| format!(".p{i}{{background:{c}}}.u{i}{{--u:{c}}}", c = u.color)).collect()
    }

    fn shell(&self, content: Markup, flash_msg: Option<&str>) -> Markup {
        html! {
            style { (PreEscaped(self.people_css())) }
            a.skip href="#main" { "Saltar al contenido" }
            div.app {
                aside.side {
                    div.brand.side-brand { (brand()) }
                    // ponytail: groups by TABS index; keep in sync if TABS changes order
                    @for (title, tabs) in [("Este mes", &TABS[..3]), ("En casa", &TABS[3..5])] {
                        nav.side-nav aria-label=(title) {
                            span.side-label { (title) }
                            @for &(id, label, ic) in tabs { (self.side_link(id, label, ic)) }
                        }
                    }
                    nav.side-nav.side-bottom aria-label="Ajustes" { @let (id, label, ic) = TABS[5]; (self.side_link(id, label, ic)) }
                    div.side-user {
                        (self.avatar(self.me.id))
                        span.side-who { b { (self.me.name) } small { (self.users.len()) " en casa" } }
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

/// Sum of what was left each month from `carry_since` up to the month before the one on screen
/// (at most 36 months back). Each month is computed exactly as its own Resumen does.
/// The starting balance counts from `carry_since` on.
fn carried(c: &Connection, v: &View) -> rusqlite::Result<i64> {
    if !v.carry || v.m < v.carry_since {
        return Ok(0);
    }
    let start = std::cmp::max(v.carry_since.clone(), shift(&v.m, -36));
    let mut m = start;
    let mut total = v.carry_start;
    while m < v.m {
        let me = User { id: v.me.id, name: v.me.name.clone(), salary: v.me.salary, token: v.me.token.clone() };
        total += View::load(c, me, m.clone(), "home", String::new(), None, None)?.left();
        m = shift(&m, 1);
    }
    Ok(total)
}

/// Greedy matching of debtors to creditors, biggest first. Rounding leftovers (a few cents) are dropped.
pub fn settle_up(mut bal: Vec<(i64, i64)>) -> Vec<(i64, i64, i64)> {
    let mut out = Vec::new();
    loop {
        bal.sort_by_key(|b| b.1);
        let (d, c) = (0, bal.len().saturating_sub(1));
        if bal.len() < 2 || bal[d].1 >= -2 || bal[c].1 <= 2 {
            return out;
        }
        let amount = (-bal[d].1).min(bal[c].1);
        out.push((bal[d].0, bal[c].0, amount));
        bal[d].1 += amount;
        bal[c].1 -= amount;
    }
}

/// Each person's weight in the shared bills for month `m`, following the house's split mode.
fn weights(c: &Connection, split: &str, users: &[U], m: &str) -> rusqlite::Result<Vec<(i64, i64)>> {
    Ok(match split {
        "equal" => users.iter().map(|u| (u.id, 1)).collect(),
        "custom" => users.iter().map(|u| (u.id, u.share)).collect(),
        "avg3" => {
            let salaries: Vec<(i64, String, String, i64)> = c
                .prepare("SELECT user_id,month,day,amount FROM salary_received")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<Result<_, _>>()?;
            let received: Vec<(i64, String, i64)> = c
                .prepare("SELECT user_id,month,amount FROM incomes WHERE received IS NOT NULL")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?;
            let mut out = Vec::new();
            for u in users {
                let track: bool = c.query_row("SELECT track_pay FROM users WHERE id=?1", [u.id], |r| r.get(0))?;
                let mine_s: Vec<(String, String, i64)> = salaries.iter().filter(|s| s.0 == u.id).map(|s| (s.1.clone(), s.2.clone(), s.3)).collect();
                let mine_r: Vec<(String, i64)> = received.iter().filter(|r| r.0 == u.id).map(|r| (r.1.clone(), r.2)).collect();
                let total: i64 = (1..=3).map(|k| cash_in(&shift(m, -k), u.salary, track, &mine_s, &mine_r)).sum();
                out.push((u.id, total / 3));
            }
            out
        }
        _ => users.iter().map(|u| (u.id, u.salary)).collect(),
    })
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
    let mut v = View::load(&c, me, m, tab(q.tab.as_deref()), host, q.day.as_deref(), q.who.as_deref())?;
    if v.t == "home" {
        v.carried = carried(&c, &v)?;
    }
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
    // ponytail: only the app shell is themed; login and sheets stay on the default
    Ok(page(&format!("{} {} · Nido", v.month_name(), v.year()), &format!("theme-{}", v.theme), body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Payslip tracked since two months ago: last month's arrived late (today), the one before never did.
    #[test]
    fn owed_and_cash_basis() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c);
        let now = today(&c).unwrap()[..7].to_string();
        let (m2, m1) = (shift(&now, -2), shift(&now, -1));
        c.execute("INSERT INTO users(id,name,pass,token,salary,pay_since,track_pay) VALUES(1,'Ana','x','t',180000,?1,1)", [&m2]).unwrap();
        c.execute("INSERT INTO salary_received(user_id,month,day,amount) VALUES(1,?1,date('now','localtime'),180000)", [&m1]).unwrap();
        c.execute("INSERT INTO incomes(user_id,month,concept,amount,received) VALUES(1,?1,'Factura',50000,NULL)", [&m1]).unwrap();
        c.execute("INSERT INTO incomes(user_id,month,concept,amount,received) VALUES(1,?1,'Clases',20000,date('now','localtime'))", [&now]).unwrap();
        let me = User { id: 1, name: "Ana".into(), salary: 180000, token: "t".into() };
        let v = View::load(&c, me, now.clone(), "home", "h".into(), None, None).unwrap();

        let owed: Vec<_> = v.owed().into_iter().map(|o| (o.label, o.amount)).collect();
        let first = MONTHS[m2[5..].parse::<usize>().unwrap() - 1].to_lowercase();
        assert_eq!(owed, vec![(format!("Ingreso fijo de {first}"), 180000), ("Factura".into(), 50000)]);
        // Last month's payslip, ticked today, counts in last month; this month only has today's extra.
        assert_eq!(v.income_in(&m1), 180000);
        assert_eq!(v.income(), 20000);
        // This month's own payslip is not ticked yet, so it is not cash but it is expected.
        assert_eq!(v.salary_day(&now), None);
        assert_eq!(v.expected(), 180000 + 20000);
        // Looking at last month: only what was pending before it.
        let me = User { id: 1, name: "Ana".into(), salary: 180000, token: "t".into() };
        let back = View::load(&c, me, m1.clone(), "home", "h".into(), None, None).unwrap();
        let owed: Vec<_> = back.owed().into_iter().map(|o| o.label).collect();
        assert_eq!(owed, vec![format!("Ingreso fijo de {first}")]);
        // Confirming by hand means nothing is assumed, not even before pay_since.
        assert_eq!(v.income_in(&shift(&m2, -1)), 0);

        // Opting out: nothing is pending and the fixed income always counts.
        c.execute("UPDATE users SET track_pay=0", []).unwrap();
        let me = User { id: 1, name: "Ana".into(), salary: 180000, token: "t".into() };
        let v = View::load(&c, me, now.clone(), "home", "h".into(), None, None).unwrap();
        assert_eq!(v.owed().iter().filter(|o| o.label.starts_with("Ingreso fijo")).count(), 0);
        assert_eq!(v.income(), 180000 + 20000);
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    fn part_of(c: &Connection, user: i64) -> i64 {
        let now = today(c).unwrap()[..7].to_string();
        let me = User { id: user, name: "x".into(), salary: 0, token: "t".into() };
        View::load(c, me, now, "house", "h".into(), None, None).unwrap().part(100000, user)
    }

    /// 1.000 € of shared bills between Ana (fixed 3.000) and Leo (fixed 1.000, plus 2.000 extra each past month).
    #[test]
    fn split_modes() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c);
        let now = today(&c).unwrap()[..7].to_string();
        c.execute_batch("INSERT INTO users(id,name,pass,token,salary,share) VALUES(1,'Ana','x','a',300000,70),(2,'Leo','x','b',100000,30);").unwrap();
        for k in 1..=3 {
            c.execute("INSERT INTO incomes(user_id,month,concept,amount,received) VALUES(2,?1,'Extra',200000,?1 || '-15')", [shift(&now, -k)]).unwrap();
        }
        let set = |mode: &str| { c.execute("INSERT INTO settings(key,value) VALUES('split',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [mode]).unwrap(); };

        assert_eq!((part_of(&c, 1), part_of(&c, 2)), (75000, 25000)); // default: fixed income 3:1
        set("equal");
        assert_eq!((part_of(&c, 1), part_of(&c, 2)), (50000, 50000));
        set("custom");
        assert_eq!((part_of(&c, 1), part_of(&c, 2)), (70000, 30000));
        set("avg3"); // Ana 3.000/month, Leo 1.000 + 2.000 = 3.000/month
        assert_eq!((part_of(&c, 1), part_of(&c, 2)), (50000, 50000));
    }
}

#[cfg(test)]
mod settle_tests {
    use super::*;

    #[test]
    fn settle_up_three_people() {
        // Ana paid 300 for the three of them; Leo and Eva owe her 100 each.
        assert_eq!(settle_up(vec![(1, 20000), (2, -10000), (3, -10000)]), vec![(2, 1, 10000), (3, 1, 10000)]);
        // Chain: Leo owes 50, Eva owes 30, Ana is owed 80 → two payments to Ana.
        let mut t = settle_up(vec![(1, 8000), (2, -5000), (3, -3000)]);
        t.sort();
        assert_eq!(t, vec![(2, 1, 5000), (3, 1, 3000)]);
        // Two creditors, one debtor.
        let mut t = settle_up(vec![(1, 6000), (2, 4000), (3, -10000)]);
        t.sort();
        assert_eq!(t, vec![(3, 1, 6000), (3, 2, 4000)]);
        // Rounding dust is not a debt.
        assert!(settle_up(vec![(1, 1), (2, -1)]).is_empty());
    }
}

#[cfg(test)]
mod carry_tests {
    use super::*;

    /// Fixed 1.000/month, carried from two months ago: 1.000 - 300 spent, then 1.000 - 1.200 spent.
    #[test]
    fn carries_leftovers() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c);
        let now = today(&c).unwrap()[..7].to_string();
        let (m2, m1) = (shift(&now, -2), shift(&now, -1));
        c.execute("INSERT INTO users(id,name,pass,token,salary,carry,carry_since) VALUES(1,'Ana','x','t',100000,1,?1)", [&m2]).unwrap();
        c.execute("INSERT INTO expenses(user_id,month,description,amount,created) VALUES(1,?1,'a',30000,?1 || '-10 12:00:00')", [&m2]).unwrap();
        c.execute("INSERT INTO expenses(user_id,month,description,amount,created) VALUES(1,?1,'b',120000,?1 || '-10 12:00:00')", [&m1]).unwrap();
        let load = |m: &str| {
            let me = User { id: 1, name: "Ana".into(), salary: 100000, token: "t".into() };
            let mut v = View::load(&c, me, m.to_string(), "home", "h".into(), None, None).unwrap();
            v.carried = carried(&c, &v).unwrap();
            v
        };
        assert_eq!(load(&m2).carried, 0);
        assert_eq!(load(&m1).carried, 70000);
        let v = load(&now);
        assert_eq!(v.carried, 70000 - 20000);
        assert_eq!(v.balance(), 50000 + 100000);
        // A starting balance of 54,81 € joins from the first carried month on, not before.
        c.execute("UPDATE users SET carry_start=5481", []).unwrap();
        assert_eq!(load(&shift(&m2, -1)).carried, 0);
        assert_eq!(load(&m2).carried, 5481);
        assert_eq!(load(&now).carried, 5481 + 50000);
        // Turned off: every month starts from zero again.
        c.execute("UPDATE users SET carry=0", []).unwrap();
        assert_eq!(load(&now).carried, 0);
    }
}
