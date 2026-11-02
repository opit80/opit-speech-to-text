//! Normalizes provider language names and ISO 639 codes without guessing unknown values.

// ISO 639-1, ISO 639-2/T, English name, and optional alternate names/codes.
const LANGUAGES: &str = "aa|aar|afar
ab|abk|abkhazian|abkhaz
ae|ave|avestan
af|afr|afrikaans
ak|aka|akan
am|amh|amharic
an|arg|aragonese
ar|ara|arabic
as|asm|assamese
av|ava|avaric
ay|aym|aymara
az|aze|azerbaijani
ba|bak|bashkir
be|bel|belarusian
bg|bul|bulgarian
bh|bih|bihari
bi|bis|bislama
bm|bam|bambara
bn|ben|bengali
bo|bod|tibetan|tib
br|bre|breton
bs|bos|bosnian
ca|cat|catalan|valencian
ce|che|chechen
ch|cha|chamorro
co|cos|corsican
cr|cre|cree
cs|ces|czech|cze
cu|chu|church slavic|old church slavonic
cv|chv|chuvash
cy|cym|welsh|wel
da|dan|danish
de|deu|german|ger
dv|div|divehi|dhivehi|maldivian
dz|dzo|dzongkha
ee|ewe|ewe
el|ell|greek|gre
en|eng|english
eo|epo|esperanto
es|spa|spanish|castilian
et|est|estonian
eu|eus|basque|baq
fa|fas|persian|per|farsi
ff|ful|fulah
fi|fin|finnish
fj|fij|fijian
fo|fao|faroese
fr|fra|french|fre
fy|fry|western frisian|frisian
ga|gle|irish
gd|gla|gaelic|scottish gaelic
gl|glg|galician
gn|grn|guarani
gu|guj|gujarati
gv|glv|manx
ha|hau|hausa
he|heb|hebrew
hi|hin|hindi
ho|hmo|hiri motu
hr|hrv|croatian
ht|hat|haitian|haitian creole
hu|hun|hungarian
hy|hye|armenian|arm
hz|her|herero
ia|ina|interlingua
id|ind|indonesian
ie|ile|interlingue|occidental
ig|ibo|igbo
ii|iii|sichuan yi|nuosu
ik|ipk|inupiaq
io|ido|ido
is|isl|icelandic|ice
it|ita|italian
iu|iku|inuktitut
ja|jpn|japanese
jv|jav|javanese
ka|kat|georgian|geo
kg|kon|kongo
ki|kik|kikuyu|gikuyu
kj|kua|kuanyama|kwanyama
kk|kaz|kazakh
kl|kal|kalaallisut|greenlandic
km|khm|central khmer|khmer
kn|kan|kannada
ko|kor|korean
kr|kau|kanuri
ks|kas|kashmiri
ku|kur|kurdish
kv|kom|komi
kw|cor|cornish
ky|kir|kirghiz|kyrgyz
la|lat|latin
lb|ltz|luxembourgish|letzeburgesch
lg|lug|ganda|luganda
li|lim|limburgan|limburgish
ln|lin|lingala
lo|lao|lao
lt|lit|lithuanian
lu|lub|luba-katanga
lv|lav|latvian
mg|mlg|malagasy
mh|mah|marshallese
mi|mri|maori|mao
mk|mkd|macedonian|mac
ml|mal|malayalam
mn|mon|mongolian
mr|mar|marathi
ms|msa|malay|may
mt|mlt|maltese
my|mya|burmese|bur
na|nau|nauru
nb|nob|norwegian bokmal|norwegian bokmål
nd|nde|north ndebele
ne|nep|nepali
ng|ndo|ndonga
nl|nld|dutch|flemish|dut
nn|nno|norwegian nynorsk|nynorsk
no|nor|norwegian
nr|nbl|south ndebele
nv|nav|navajo|navaho
ny|nya|chichewa|chewa|nyanja
oc|oci|occitan
oj|oji|ojibwa
om|orm|oromo
or|ori|oriya|odia
os|oss|ossetian|ossetic
pa|pan|panjabi|punjabi
pi|pli|pali
pl|pol|polish
ps|pus|pushto|pashto
pt|por|portuguese
qu|que|quechua
rm|roh|romansh
rn|run|rundi|kirundi
ro|ron|romanian|moldavian|moldovan|rum
ru|rus|russian
rw|kin|kinyarwanda
sa|san|sanskrit
sc|srd|sardinian
sd|snd|sindhi
se|sme|northern sami
sg|sag|sango
si|sin|sinhala|sinhalese
sk|slk|slovak|slo
sl|slv|slovenian
sm|smo|samoan
sn|sna|shona
so|som|somali
sq|sqi|albanian|alb
sr|srp|serbian
ss|ssw|swati
st|sot|southern sotho|sotho
su|sun|sundanese
sv|swe|swedish
sw|swa|swahili
ta|tam|tamil
te|tel|telugu
tg|tgk|tajik
th|tha|thai
ti|tir|tigrinya
tk|tuk|turkmen
tl|tgl|tagalog|filipino|fil
tn|tsn|tswana
to|ton|tonga
tr|tur|turkish
ts|tso|tsonga
tt|tat|tatar
tw|twi|twi
ty|tah|tahitian
ug|uig|uighur|uyghur
uk|ukr|ukrainian
ur|urd|urdu
uz|uzb|uzbek
ve|ven|venda
vi|vie|vietnamese
vo|vol|volapuk|volapük
wa|wln|walloon
wo|wol|wolof
xh|xho|xhosa
yi|yid|yiddish
yo|yor|yoruba
za|zha|zhuang
zh|zho|chinese|chi|mandarin
zu|zul|zulu";

pub fn normalize(value: &str) -> Option<String> {
    let value = value.trim().to_lowercase();
    LANGUAGES.lines().find_map(|line| {
        let mut fields = line.split('|');
        let code = fields.next()?;
        (value == code || fields.any(|alias| alias == value)).then(|| code.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_two_and_three_letter_codes_normalize_without_guessing() {
        for (input, expected) in [
            (" Turkish ", "tr"),
            ("TR", "tr"),
            ("tur", "tr"),
            ("English", "en"),
            ("eng", "en"),
            ("deu", "de"),
            ("ger", "de"),
            ("FRENCH", "fr"),
            ("zho", "zh"),
            ("fil", "tl"),
        ] {
            assert_eq!(normalize(input).as_deref(), Some(expected));
        }
        for value in ["auto", "", "unknown", "zzz", "xx", "english-ish"] {
            assert_eq!(normalize(value), None);
        }
        for line in LANGUAGES.lines() {
            let code = line.split('|').next().unwrap();
            for alias in line.split('|') {
                assert_eq!(normalize(alias).as_deref(), Some(code));
            }
        }
    }
}
