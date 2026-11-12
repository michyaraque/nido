use crate::merchants::fold;

const GROUPS: &[(&str, &str, &str)] = &[
    ("milk", "#3b82f6", "leche yogur yogurt queso mantequilla nata kefir batido griego cuajada"),
    ("egg", "#d97706", "huevo"),
    ("croissant", "#b45309", "pan baguette barra croissant bolleria magdalena tostada bizcocho"),
    ("cookie", "#92400e", "galleta chocolate cacao cereal muesli"),
    ("citrus", "#f59e0b", "naranja limon mandarina pomelo lima"),
    ("banana", "#eab308", "platano banana"),
    ("apple", "#16a34a", "manzana pera fruta melocoton kiwi uva fresa sandia melon piña aguacate"),
    ("carrot", "#ea580c", "zanahoria tomate patata cebolla ajo pimiento calabacin berenjena verdura brocoli pepino"),
    ("salad", "#22c55e", "lechuga ensalada espinaca cogollo rucula canonigo acelga"),
    ("beef", "#dc2626", "carne ternera cerdo filete hamburguesa secreto lomo costilla picada solomillo"),
    ("drumstick", "#ea580c", "pollo pavo jamoncito alita muslo pechuga"),
    ("ham", "#e11d48", "jamon chorizo salchichon fuet embutido bacon loncha salchicha mortadela"),
    ("fish", "#0891b2", "pescado atun salmon merluza bacalao gamba langostino mejillon sardina calamar"),
    ("wheat", "#a16207", "arroz pasta macarron espagueti harina avena legumbre lenteja garbanzo alubia quinoa"),
    ("coffee", "#78350f", "cafe te infusion capsula"),
    ("wine", "#9f1239", "vino cava sidra"),
    ("beer", "#ca8a04", "cerveza"),
    ("cup-soda", "#e11d48", "refresco cola zumo tonica"),
    ("droplets", "#0ea5e9", "agua aceite vinagre"),
    ("candy", "#db2777", "chuche caramelo gominola"),
    ("ice-cream-cone", "#ec4899", "helado polo"),
    ("pizza", "#ea580c", "pizza"),
    ("baby", "#f472b6", "pañal panal toallita papilla potito"),
    ("paw-print", "#d97706", "pienso perro gato arena mascota"),
    ("spray-can", "#6366f1", "lejia detergente suavizante limpiador friegasuelos lavavajillas amoniaco estropajo bayeta"),
    ("bath", "#0ea5e9", "gel champu jabon desodorante dentifrico cepillo compresa crema maquinilla"),
    ("package", "#64748b", "papel aluminio film bolsa servilleta pila bombilla"),
    ("pill", "#10b981", "paracetamol ibuprofeno tirita medicina"),
];

pub fn look(name: &str) -> (&'static str, &'static str) {
    let folded = fold(name);
    let same = |w: &str, kw: &str| w == kw || w.strip_suffix('s') == Some(kw) || w.strip_suffix("es") == Some(kw);
    folded
        .split_whitespace()
        .find_map(|w| GROUPS.iter().find(|(_, _, kws)| kws.split_whitespace().any(|kw| same(w, fold(kw).trim()))))
        .map(|(icon, color, _)| (*icon, *color))
        .unwrap_or(("shopping-basket", "#64748b"))
}

#[cfg(test)]
mod tests {
    use super::look;

    #[test]
    fn guesses_product_icons() {
        assert_eq!(look("Leche entera").0, "milk");
        assert_eq!(look("Huevos L").0, "egg");
        assert_eq!(look("TOMATES pera").0, "carrot");
        assert_eq!(look("Jamón cocido").0, "ham");
        assert_eq!(look("Pañales talla 4").0, "baby");
        assert_eq!(look("Algo raro").0, "shopping-basket");
    }
}
