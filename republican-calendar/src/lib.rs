use std::convert::TryFrom;

use time::Date;

#[allow(dead_code)] // never constructed because I'm using the mapping to u8
#[derive(Eq, PartialEq, Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Month {
    Vnd = 0,
    Bru = 1,
    Fri = 2,
    Niv = 3,
    Plu = 4,
    Vnt = 5,
    Ger = 6,
    Flo = 7,
    Pra = 8,
    Mes = 9,
    The = 10,
    Fru = 11,
    SC = 12,
}

impl std::fmt::Display for Month {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let res = match self {
            Month::Vnd => "Vendémiaire",
            Month::Bru => "Brumaire",
            Month::Fri => "Frimaire",
            Month::Niv => "Nivôse",
            Month::Plu => "Pluviôse",
            Month::Vnt => "Ventôse",
            Month::Ger => "Germinal",
            Month::Flo => "Floréal",
            Month::Pra => "Prairial",
            Month::Mes => "Messidor",
            Month::The => "Thermidor",
            Month::Fru => "Fructidor",
            Month::SC => "Sans-Culottides",
        };
        write!(f, "{}", res)
    }
}

impl TryFrom<u8> for Month {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value <= 12 {
            Ok(unsafe { std::mem::transmute(value) })
        } else {
            Err("Month cannot be strictly greater than 12")
        }
    }
}

#[derive(Eq, PartialEq, Debug)]
pub struct RepublicanDate {
    year: i32,
    month: Month,
    day: u8,
}

impl std::fmt::Display for RepublicanDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} − jour {} − et c'est un {}",
            self.day,
            self.month,
            self.year,
            self.day_symbol(),
            self.day_name(),
        )
    }
}

// Everything that follows is more or less a translation of
// https://github.com/caarmen/french-revolutionary-calendar
// but with less features (at least for now)

impl RepublicanDate {
    fn from_yd(y: i32, day_of_year: i64) -> Result<Self, &'static str> {
        let raw_m = day_of_year / 30;
        let month = Month::try_from(raw_m as u8)?; // .context(format!("cannot find a month for {}", raw_m))?;
        let day = day_of_year - raw_m * 30 + 1; // 0 based
        Ok(RepublicanDate {
            year: y,
            month,
            day: day as _,
        })
    }

    /// Day of the week
    pub fn day_name(&self) -> &'static str {
        match self.day % 10 {
            1 => "Primedi",
            2 => "Duodi",
            3 => "Tridi",
            4 => "Quartidi",
            5 => "Quintidi",
            6 => "Sextidi",
            7 => "Septidi",
            8 => "Octidi",
            9 => "Nonidi",
            0 => "Décadi",
            _ => unreachable!(),
        }
    }

    /// name of a plant, tool, animal or symbol of the peasan world associated
    /// to this day
    pub fn day_symbol(&self) -> &'static str {
        use Month::*;
        match (self.month, self.day) {
            (Vnd, 1) => "du raisin",
            (Vnd, 2) => "du safran",
            (Vnd, 3) => "de la châtaigne",
            (Vnd, 4) => "de la colchique",
            (Vnd, 5) => "du cheval",
            (Vnd, 6) => "de la balsamine",
            (Vnd, 7) => "de la carotte",
            (Vnd, 8) => "de l'amaranthe",
            (Vnd, 9) => "du panais",
            (Vnd, 10) => "de la cuve",
            (Vnd, 11) => "de la pomme de terre",
            (Vnd, 12) => "de l'immortelle",
            (Vnd, 13) => "du potiron",
            (Vnd, 14) => "de la réséda",
            (Vnd, 15) => "de l'âne",
            (Vnd, 16) => "de la belle de nuit",
            (Vnd, 17) => "de la citrouille",
            (Vnd, 18) => "du sarrasin",
            (Vnd, 19) => "du tournesol",
            (Vnd, 20) => "du pressoir",
            (Vnd, 21) => "du chanvre",
            (Vnd, 22) => "de la pêche",
            (Vnd, 23) => "du navet",
            (Vnd, 24) => "de l'amaryllis",
            (Vnd, 25) => "du bœuf",
            (Vnd, 26) => "de l'aubergine",
            (Vnd, 27) => "du piment",
            (Vnd, 28) => "de la tomate",
            (Vnd, 29) => "de l'orge",
            (Vnd, 30) => "du tonneau",
            (Bru, 1) => "de la pomme",
            (Bru, 2) => "du céleri",
            (Bru, 3) => "de la poire",
            (Bru, 4) => "de la betterave",
            (Bru, 5) => "de l'oie",
            (Bru, 6) => "de l'héliotrope",
            (Bru, 7) => "de la figue",
            (Bru, 8) => "de la scorsonère",
            (Bru, 9) => "de l'alisier",
            (Bru, 10) => "de la charrue",
            (Bru, 11) => "du salsifis",
            (Bru, 12) => "de la mâcre",
            (Bru, 13) => "du topinambour",
            (Bru, 14) => "de l'endive",
            (Bru, 15) => "du dindon",
            (Bru, 16) => "du chervis",
            (Bru, 17) => "du cresson",
            (Bru, 18) => "de la dentelaire",
            (Bru, 19) => "de la grenade",
            (Bru, 20) => "de la herse",
            (Bru, 21) => "de la bacchante",
            (Bru, 22) => "de l'azerole",
            (Bru, 23) => "de la garance",
            (Bru, 24) => "de l'orange",
            (Bru, 25) => "du faisan",
            (Bru, 26) => "de la pistache",
            (Bru, 27) => "du macjonc",
            (Bru, 28) => "du coing",
            (Bru, 29) => "du cormier",
            (Bru, 30) => "du rouleau",
            (Fri, 1) => "de la raiponce",
            (Fri, 2) => "du turneps",
            (Fri, 3) => "de la chicorée",
            (Fri, 4) => "du nèfle",
            (Fri, 5) => "du cochon",
            (Fri, 6) => "de la mâche",
            (Fri, 7) => "du chou-fleur",
            (Fri, 8) => "du miel",
            (Fri, 9) => "de la genièvre",
            (Fri, 10) => "de la pioche",
            (Fri, 11) => "de la cire",
            (Fri, 12) => "de la cire",
            (Fri, 13) => "du cèdre",
            (Fri, 14) => "du sapin",
            (Fri, 15) => "du chevreuil",
            (Fri, 16) => "de l'ajonc",
            (Fri, 17) => "du cyprès",
            (Fri, 18) => "du lierre",
            (Fri, 19) => "de la sabine",
            (Fri, 20) => "du hoyau",
            (Fri, 21) => "de l'érable à sucre",
            (Fri, 22) => "de la bruyère",
            (Fri, 23) => "du roseau",
            (Fri, 24) => "de l'oseille",
            (Fri, 25) => "du grillon",
            (Fri, 26) => "du pignon",
            (Fri, 27) => "de la liège",
            (Fri, 28) => "de la truffe",
            (Fri, 29) => "de l'olive",
            (Fri, 30) => "de la pelle",
            (Niv, 1) => "de la tourbe",
            (Niv, 2) => "de la houille",
            (Niv, 3) => "du bitume",
            (Niv, 4) => "du soufre",
            (Niv, 5) => "du chien",
            (Niv, 6) => "de la lave",
            (Niv, 7) => "de la terre végétale",
            (Niv, 8) => "du fumier",
            (Niv, 9) => "du salpêtre",
            (Niv, 10) => "du fléau",
            (Niv, 11) => "du granit",
            (Niv, 12) => "de l'argile",
            (Niv, 13) => "de l'ardoise",
            (Niv, 14) => "du grès",
            (Niv, 15) => "du lapin",
            (Niv, 16) => "du silex",
            (Niv, 17) => "de la marne",
            (Niv, 18) => "de la pierre à chaux",
            (Niv, 19) => "du marbre",
            (Niv, 20) => "du van",
            (Niv, 21) => "de la pierre à plâtre",
            (Niv, 22) => "du sel",
            (Niv, 23) => "du fer",
            (Niv, 24) => "du cuivre",
            (Niv, 25) => "du chat",
            (Niv, 26) => "de l'étain",
            (Niv, 27) => "du plomb",
            (Niv, 28) => "du zinc",
            (Niv, 29) => "du mercure",
            (Niv, 30) => "du crible",
            (Plu, 1) => "de lauréole",
            (Plu, 2) => "de la mousse",
            (Plu, 3) => "du fragon",
            (Plu, 4) => "du perce-neige",
            (Plu, 5) => "du taureau",
            (Plu, 6) => "du laurier-tin",
            (Plu, 7) => "de l'amadouvier",
            (Plu, 8) => "du mézéréon",
            (Plu, 9) => "du peuplier",
            (Plu, 10) => "de la cognée",
            (Plu, 11) => "de l'ellébore",
            (Plu, 12) => "du brocoli",
            (Plu, 13) => "du laurier",
            (Plu, 14) => "de l'avelinier",
            (Plu, 15) => "de la vache",
            (Plu, 16) => "du buis",
            (Plu, 17) => "du lichen",
            (Plu, 18) => "de l'if",
            (Plu, 19) => "de la pulmonaire",
            (Plu, 20) => "de la serpette",
            (Plu, 21) => "de la thlaspi",
            (Plu, 22) => "du thimele",
            (Plu, 23) => "du chiendent",
            (Plu, 24) => "de la trainasse",
            (Plu, 25) => "du lièvre",
            (Plu, 26) => "du guède",
            (Plu, 27) => "du noisetier",
            (Plu, 28) => "de la cyclamen",
            (Plu, 29) => "de la chélidoine",
            (Plu, 30) => "du traîneau",
            (Vnt, 1) => "du tussilage",
            (Vnt, 2) => "du cornouiller",
            (Vnt, 3) => "du violier",
            (Vnt, 4) => "du troène",
            (Vnt, 5) => "du bouc",
            (Vnt, 6) => "des asaret",
            (Vnt, 7) => "de l'alaterne",
            (Vnt, 8) => "de la violette",
            (Vnt, 9) => "du marceau",
            (Vnt, 10) => "de la bêche",
            (Vnt, 11) => "de la narcisse",
            (Vnt, 12) => "de l'orme",
            (Vnt, 13) => "des fumeterre",
            (Vnt, 14) => "de la vélar",
            (Vnt, 15) => "de la chèvre",
            (Vnt, 16) => "de l'épinard",
            (Vnt, 17) => "de la doronic",
            (Vnt, 18) => "du mouron",
            (Vnt, 19) => "du cerfeuil",
            (Vnt, 20) => "du cordeau",
            (Vnt, 21) => "de la mandragore",
            (Vnt, 22) => "du persil",
            (Vnt, 23) => "du cochléaria",
            (Vnt, 24) => "de la pâquerette",
            (Vnt, 25) => "du thon",
            (Vnt, 26) => "du pissenlit",
            (Vnt, 27) => "de la sylvie",
            (Vnt, 28) => "de la capillaire",
            (Vnt, 29) => "du frêne",
            (Vnt, 30) => "du plantoir",
            (Ger, 1) => "de la primevère",
            (Ger, 2) => "du platane",
            (Ger, 3) => "de l'asperge",
            (Ger, 4) => "de la tulipe",
            (Ger, 5) => "de la poule",
            (Ger, 6) => "de la bette",
            (Ger, 7) => "du bouleau",
            (Ger, 8) => "de la jonquille",
            (Ger, 9) => "de l'aulne",
            (Ger, 10) => "du couvoir",
            (Ger, 11) => "de la pervenche",
            (Ger, 12) => "du charme",
            (Ger, 13) => "de la morille",
            (Ger, 14) => "de l'hêtre",
            (Ger, 15) => "de l'abeille",
            (Ger, 16) => "de la laitue",
            (Ger, 17) => "du mélèze",
            (Ger, 18) => "de la ciguë",
            (Ger, 19) => "du radis",
            (Ger, 20) => "de la ruche",
            (Ger, 21) => "du gainier",
            (Ger, 22) => "de la romaine",
            (Ger, 23) => "du marronnier",
            (Ger, 24) => "de la roquette",
            (Ger, 25) => "du pigeon",
            (Ger, 26) => "du lilas",
            (Ger, 27) => "de l'anémone",
            (Ger, 28) => "de la pensée",
            (Ger, 29) => "de la myrtille",
            (Ger, 30) => "du greffoir",
            (Flo, 1) => "de la rose",
            (Flo, 2) => "du chêne",
            (Flo, 3) => "de la fougère",
            (Flo, 4) => "de l'aubépine",
            (Flo, 5) => "du rossignol",
            (Flo, 6) => "de l'ancolie",
            (Flo, 7) => "du muguet",
            (Flo, 8) => "du champignon",
            (Flo, 9) => "de la hyacinthe",
            (Flo, 10) => "du râteau",
            (Flo, 11) => "de la rhubarbe",
            (Flo, 12) => "du sainfoin",
            (Flo, 13) => "du bâton-d'or",
            (Flo, 14) => "du chamérisier",
            (Flo, 15) => "du ver à soie",
            (Flo, 16) => "de la consoude",
            (Flo, 17) => "de la pimprenelle",
            (Flo, 18) => "de la corbeille d'or",
            (Flo, 19) => "de l'arroche",
            (Flo, 20) => "du sarcloir",
            (Flo, 21) => "de la statice",
            (Flo, 22) => "de la fritillaire",
            (Flo, 23) => "de la bourrache",
            (Flo, 24) => "de la valériane",
            (Flo, 25) => "de la carpe",
            (Flo, 26) => "du fusain",
            (Flo, 27) => "de la civette",
            (Flo, 28) => "de la buglosse",
            (Flo, 29) => "de la sénevé",
            (Flo, 30) => "de la houlette",
            (Pra, 1) => "de la luzerne",
            (Pra, 2) => "de l'hémérocalle",
            (Pra, 3) => "du trèfle",
            (Pra, 4) => "de l'angélique",
            (Pra, 5) => "du canard",
            (Pra, 6) => "de la mélisse",
            (Pra, 7) => "du fromental",
            (Pra, 8) => "du martagon",
            (Pra, 9) => "du serpolet",
            (Pra, 10) => "de la faux",
            (Pra, 11) => "de la fraise",
            (Pra, 12) => "de la bétoine",
            (Pra, 13) => "du pois",
            (Pra, 14) => "de l'acacia",
            (Pra, 15) => "de la caille",
            (Pra, 16) => "de l'œillet",
            (Pra, 17) => "du sureau",
            (Pra, 18) => "du pavot",
            (Pra, 19) => "du tilleul",
            (Pra, 20) => "de la fourche",
            (Pra, 21) => "du barbeau",
            (Pra, 22) => "de la camomille",
            (Pra, 23) => "du chèvrefeuille",
            (Pra, 24) => "de la caille-lait",
            (Pra, 25) => "de la tanche",
            (Pra, 26) => "du jasmin",
            (Pra, 27) => "de la verveine",
            (Pra, 28) => "du thym",
            (Pra, 29) => "de la pivoine",
            (Pra, 30) => "du chariot",
            (Mes, 1) => "du seigle",
            (Mes, 2) => "de l'avoine",
            (Mes, 3) => "de l'oignon",
            (Mes, 4) => "de la véronique",
            (Mes, 5) => "du mulet",
            (Mes, 6) => "du romarin",
            (Mes, 7) => "du concombre",
            (Mes, 8) => "de l'échalote",
            (Mes, 9) => "de l'absinthe",
            (Mes, 10) => "de la faucille",
            (Mes, 11) => "de la coriandre",
            (Mes, 12) => "de l'artichaut",
            (Mes, 13) => "de la giroflée",
            (Mes, 14) => "de la lavande",
            (Mes, 15) => "du chamois",
            (Mes, 16) => "du tabac",
            (Mes, 17) => "de la groseille",
            (Mes, 18) => "de la gesse",
            (Mes, 19) => "de la cerise",
            (Mes, 20) => "du parc",
            (Mes, 21) => "de la menthe",
            (Mes, 22) => "du cumin",
            (Mes, 23) => "du haricot",
            (Mes, 24) => "de l'orcanète",
            (Mes, 25) => "de la pintade",
            (Mes, 26) => "de la sauge",
            (Mes, 27) => "de l'ail",
            (Mes, 28) => "de la vesce",
            (Mes, 29) => "du blé",
            (Mes, 30) => "de la chalemie",
            (The, 1) => "de l'épeautre",
            (The, 2) => "du bouillon-blanc",
            (The, 3) => "du melon",
            (The, 4) => "de l'ivraie",
            (The, 5) => "du bélier",
            (The, 6) => "de la prêle",
            (The, 7) => "de l'armoise",
            (The, 8) => "de la carthame",
            (The, 9) => "de la mûre",
            (The, 10) => "de l'arrosoir",
            (The, 11) => "de la panic",
            (The, 12) => "de la salicorne",
            (The, 13) => "de l'abricot",
            (The, 14) => "du basilic",
            (The, 15) => "de la brebis",
            (The, 16) => "de la guimauve",
            (The, 17) => "du lin",
            (The, 18) => "de l'amande",
            (The, 19) => "de la gentiane",
            (The, 20) => "de l'écluse",
            (The, 21) => "de la carline",
            (The, 22) => "du câprier",
            (The, 23) => "de la lentille",
            (The, 24) => "de l'aunée",
            (The, 25) => "de la loutre",
            (The, 26) => "de la myrte",
            (The, 27) => "du colza",
            (The, 28) => "du lupin",
            (The, 29) => "du coton",
            (The, 30) => "du moulin",
            (Fru, 1) => "de la prune",
            (Fru, 2) => "du millet",
            (Fru, 3) => "du lycoperdon",
            (Fru, 4) => "de l'escourgeon",
            (Fru, 5) => "du saumon",
            (Fru, 6) => "de la tubéreuse",
            (Fru, 7) => "du sucrion",
            (Fru, 8) => "de l'apocyn",
            (Fru, 9) => "de la réglisse",
            (Fru, 10) => "de l'échelle",
            (Fru, 11) => "de la pastèque",
            (Fru, 12) => "du fenouil",
            (Fru, 13) => "de l'épine-vinette",
            (Fru, 14) => "de la noix",
            (Fru, 15) => "de la truite",
            (Fru, 16) => "du citron",
            (Fru, 17) => "de la cardère",
            (Fru, 18) => "du nerprun",
            (Fru, 19) => "de la tagette",
            (Fru, 20) => "de la hotte",
            (Fru, 21) => "de l'églantier",
            (Fru, 22) => "de la noisette",
            (Fru, 23) => "du houblon",
            (Fru, 24) => "du sorgho",
            (Fru, 25) => "de l'écrevisse",
            (Fru, 26) => "de la bigarade",
            (Fru, 27) => "de la verge d'or",
            (Fru, 28) => "du maïs",
            (Fru, 29) => "du marron",
            (Fru, 30) => "du panier",
            (SC, 1) => "la fête de la vertu",
            (SC, 2) => "la fête du génie",
            (SC, 3) => "la fête du travail",
            (SC, 4) => "la fête de l'opinion",
            (SC, 5) => "la fête des récompenses",
            (SC, 6) => "la fête de la révolution",
            _ => "ERROR",
        }
    }

}

impl TryFrom<Date> for RepublicanDate {
    type Error = &'static str;

    fn try_from(value: Date) -> Result<Self, Self::Error> {
        let french_era_end =
            Date::from_calendar_date(1811, time::Month::September, 23).map_err(|e| e.name())?;
        let duration_since_french_era_end = value - french_era_end;
        if duration_since_french_era_end.is_negative() {
            return Err("Can only convert date from after the official end of the calendar");
        }

        // create a fake Date object so we can perform conversion on it
        // and then extract the year and day of year. In the republican calendar
        // the last year was 20, but at that time, there was no leap year yet, so
        // artificially pad it.
        let padding = 2000;
        let fake_french_date = Date::from_calendar_date(20 + padding, time::Month::January, 1)
            .map_err(|e| e.name())?;
        let french_date = fake_french_date + duration_since_french_era_end;
        let tmp_date =
            Date::from_calendar_date(french_date.year(), time::Month::January, 1).unwrap();
        let day_of_year = (french_date - tmp_date).whole_days();

        RepublicanDate::from_yd(french_date.year() - padding, day_of_year)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_conversion() {
        assert_eq!(
            RepublicanDate::try_from(
                Date::from_calendar_date(2021, time::Month::January, 14).unwrap()
            ),
            Ok(RepublicanDate {
                year: 229,
                month: Month::Niv,
                day: 25
            })
        );
    }
}
