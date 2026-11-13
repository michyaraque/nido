use super::*;

pub fn tab(t: Option<&str>) -> &'static str {
    match t {
        Some("house") => "house",
        Some("expenses") => "expenses",
        Some("shopping") => "shopping",
        Some("tasks") => "tasks",
        Some("settings") => "settings",
        Some("household") => "household",
        _ => "home",
    }
}

pub fn flash(code: &str) -> Option<&'static str> {
    Some(match code {
        "expense_added" => "Gasto añadido",
        "expense_deleted" => "Gasto borrado",
        "expense_updated" => "Gasto actualizado",
        "photo_saved" => "Foto de perfil guardada",
        "photo_removed" => "Foto de perfil quitada",
        "paid" => "Pago actualizado",
        "bill_added" => "Cuenta añadida",
        "bill_updated" => "Cuenta actualizada",
        "bill_ended" => "Cuenta quitada desde este mes",
        "bill_paid_whole" => "Pagada entera por ti: este mes no se reparte",
        "bill_split_again" => "Vuelve a repartirse este mes",
        "salary" => "Sueldo guardado",
        "settled" => "Deuda saldada",
        "unsettled" => "Devolución borrada",
        "items_added" => "Añadido a la lista",
        "item_deleted" => "Quitado de la lista",
        "list_cleared" => "Lista vaciada: lo comprado queda como sugerencia",
        "ticket_saved" => "Ticket registrado como gasto",
        "ticket_deleted" => "Ticket descartado",
        "chore_added" => "Tarea creada",
        "chore_updated" => "Tarea actualizada",
        "chore_archived" => "Tarea quitada",
        "chore_logged" => "Apuntado para hoy",
        "chore_logged_day" => "Apuntado en el calendario",
        "log_deleted" => "Borrado del calendario",
        "plan_added" => "Tarea planificada",
        "plan_taken" => "Es tuya: ya te toca a ti",
        "plan_deleted" => "Quitada del calendario",
        "house_saved" => "Ajustes de la casa guardados",
        "member_added" => "Persona añadida: ya puede entrar",
        "color_saved" => "Color guardado",
        "password_saved" => "Contraseña cambiada",
        "tg_connected" => "Bot de Telegram conectado",
        "backup_pass" => "Contraseña de las copias guardada",
        "backup_time" => "Hora de las copias guardada",
        "backup_target_added" => "Destino añadido",
        "backup_target_removed" => "Destino quitado",
        "backup_target_saved" => "Destino guardado",
        "backup_copy_deleted" => "Copia borrada",
        "backup_target_ok" => "Funciona: se puede guardar y borrar",
        "backup_started" => "Copia en marcha: tarda unos segundos",
        "tg_removed" => "Bot de Telegram quitado",
        "tg_time" => "Hora del recordatorio guardada",
        "tg_test" => "Mensaje de prueba enviado",
        "tg_unlinked" => "Ya no recibirás avisos en Telegram",
        _ => return None,
    })
}

pub fn back(m: &str, t: Option<&str>, ok: &str) -> R {
    Ok(Redirect::to(&format!("/?month={m}&tab={}&ok={ok}", tab(t))).into_response())
}
