# Musha shugyō — spildesign

> Arbejdsdokument · 31. oktober 2026  
> Status: Besluttede principper, foreløbige mekanikker og åbne spørgsmål.

## 1. Vision

Et roligt, turbaseret udforskningsspil inspireret af **武者修行 (musha shugyō)**: krigerens vandring, observation, øvelse og personlige udvikling.

- **Mindre action, mere vandring og mestring.**
- Rejsen og møderne er vigtigere end antallet af besejrede fjender.
- En god spildag kan foregå helt uden kamp.
- Undvigelse, observation og fredelig passage skal være meningsfulde valg.
- Spillet må gerne have korte **cutscenes**, eksempelvis ved særlige møder og helligdomme.
- Æstetikken er inspireret af japanske skove, stier og små helligdomme. Undgå unødvendigt meget lore.

## 2. Verden og rum

- Verden er **proceduralt genereret** fra et deterministisk seed.
- Den består af et **grid af rum/områder**, som spilleren bevæger sig mellem og indenfor.
- Et rum kan indeholde vægge, træer, buske, andre hindringer, ressourcer, skatte og væsener.
- Terræn påvirker både **bevægelse** og **line of sight (LoS)**.
- Det skal være muligt at navigere uden om væsener og undgå konfrontation.
- Verden er **persistent**: indsamlede ressourcer, fund, ændringer og møder nulstilles ikke blot ved spillerens nederlag.

## 3. Månedens verden

- Der dannes et **nyt fælles seed hver måned**, knyttet til **år og måned** (fx `2026-10`).
- Alle, der starter en rejse i samme måned, får **samme oprindelige verden**: geografi, væsener, skatte, ressourcer og helligdomme.
- Hver spiller har derefter **sin egen uafhængige, persistente udgave** af verdenen. Deres handlinger skaber forskellene.
- En rejse fortsætter efter månedsskiftet; der er **ingen deadline** for at gennemføre den.
- Spillere kan dele fund og fortællinger fra samme måneds verden uden at være i multiplayer.
- Verdenens hemmeligheder skal opdages, ikke automatisk afsløres på et komplet kort.
- Gem både **seed og generatorversion**, så ændringer i genereringsalgoritmen ikke ændrer eksisterende verdener.

## 4. Tid og samtidige ture

- Spillet er **turbaseret**, men **alle aktører handler samtidig** i hver tur.
- Spillets tid går frem, når spilleren vælger en handling — også når handlingen er at **vente**.
- Alle aktører beslutter deres handling ud fra tilstanden **før** turens udførelse.
- Derefter afvikles handlingerne og deres konsekvenser samlet.
- En aktør må ikke vælge sin handling på baggrund af, hvad en anden aktør først gør i den samme tur.
- Ingen løbende bevægelsestimer tvinger spilleren til at handle hurtigt.

## 5. Zoner, syn og møder

Væsener reagerer på spillerens tilstedeværelse gennem afstand, synlighed og deres egen adfærd.

- **Zone of interest:** Væsenet bliver opmærksomt eller nysgerrigt, men udfordrer ikke nødvendigvis direkte.
- **Zone of combat:** Et synligt væsen højst to felter væk kan udfordres til kamp. Kamp startes bevidst af spilleren og udløses ikke automatisk af nærhed.
- **Line of sight:** Hindringer og terræn afgør, om parterne kan se hinanden.
- Zoner er **udløsere for vurdering**, ikke automatiske aggressionstriggere.
- Et væsens temperament, erfaring og relation til spilleren påvirker reaktionen.
- To ikke-aggressive aktører, der forsøger at passere hinanden, skal kunne **give plads høfligt** frem for automatisk at starte kamp. Den japanske paraplygestus er en oplagt lille animation.

## 6. Gensidig erfaring og opmærksomhed

- **Begge parter lærer af møder**: spilleren om væsenet, og væsenet om spilleren.
- Observation uden at blive set kan give spilleren en informationsfordel.
- Når begge ser eller bemærker hinanden, kan begge opnå erfaring.
- **Lige stor relevant erfaring udligner informationsfordelen.**
- At blive opdaget og være nødt til at flygte er derfor ikke nødvendigvis en fordel, selv hvis spilleren lærer noget.
- Gentagen vellykket undvigelse af **samme væsen** kan mindske dets opmærksomhed på spilleren, afhængigt af hvad væsenet selv har oplevet.
- Et væsen kan lære, at spilleren er ufarlig, interessant, truende eller tilbøjelig til at flygte.
- Erfaring skal primært komme til udtryk i **adfærd og muligheder**, ikke nødvendigvis som synlige pointtal.
- Erfaring med én bestemt aktør er ikke automatisk det samme som generel kampstyrke.

## 7. Hokora og checkpoints

En **hokora (祠)**, en lille helligdom, fungerer som checkpoint.

- Et checkpoint **forankrer spillerens erfaringer**.
- Det gemmer også spillerens relevante tilstand, herunder position, udstyr og beholdning.
- **Kun det senest aktiverede checkpoint** kan bruges til at vende tilbage til.
- Tidligere helligdomme kan stadig eksistere i verdenen, men er ikke længere aktive som tilbagegangspunkt.
- Ved et alvorligt nederlag vender spilleren tilbage til den **senest forankrede spillertilstand**.
- **Verdens tilstand fortsætter uændret** af denne tilbagerulning: fundne eller fjernede ressourcer, påvirkede væsener og andre konsekvenser består.
- Derfor skal værdier og fund **bruges, gemmes sikkert eller bringes til et checkpoint**, hvis man vil sikre dem.
- En kort, rolig cutscene ved aktivering af hokora passer til spillets stemning.

## 8. Væsener og vedvarende konsekvenser

- Væsener er ikke blot engangsfjender: de **husker møder** og kan mødes igen.
- Et delvist besejret eller såret væsen kan **flygte eller gemme sig** i området.
- Væsener kan **hele over tid** og blive friske igen.
- Deres restitution kan afhænge af **områdets føde**, fx svampe og bær.
- Spilleren kan påvirke et væsens muligheder for at komme sig ved at indsamle områdets ressourcer.
- Ressourcer kan med tiden komme igen; væsener kan muligvis søge føde andre steder.
- Et område, man forlader, behøver derfor ikke være i samme tilstand, når man vender tilbage.
- Verden skal føles levende uden nødvendigvis at simulere et detaljeret økosystem.

## 9. Nederlag, flugt og forvisning

- Møder og kampe skal have **flere mulige udfald** end blot sejr eller død.
- Væsener kan flygte, gemme sig, overgive plads eller fortsætte et senere møde.
- Spilleren kan blive **forvist** eller drevet tilbage til et tidligere rum/område uden at miste sin aktuelle forankring.
- **Forvisning** og **alvorligt nederlag med checkpoint-retur** er forskellige konsekvenser.
- Nederlag skal have mærkbare konsekvenser, men ikke nødvendigvis være lig med død.
- Når et væsens liv når nul, flygter det fra rummet.
- Når spillerens liv når nul, forvises spilleren til seneste hokora; uden en aktiv hokora er udgangspunktet skovens indgang. Spillerens forankrede tilstand gendannes, usikrede fund mistes, og verdens tilstand rulles ikke tilbage.

## 10. Kamp, våben og progression

- **Sværd** er det naturlige udgangspunkt og muligvis det eneste våben.
- **Bue** er en mulighed, men er ikke besluttet.
- Færdigheder bør handle om fx afstand, timing, observation, forudsigelse og beherskelse — ikke kun større skadetal.
- En erfaren vandrer kan muligvis bedre **aflæse intentioner**, før en samtidig tur afvikles.
- Ikke alle møder skal kunne eller skulle løses med kamp.
- Progression skal understøtte, at spilleren lærer verdenen at kende.

### Nuværende kampregler

- Spilleren har kampfærdighed 2 og 10/10 liv fra start. Væsener har kampfærdighed 1–3 og 3–6 liv; væsenet i startområdet har kampfærdighed 2 og 4/4 liv.
- En kampudveksling slår samtidig et antal d6 svarende til hver parts kampfærdighed. Den højeste terning er angrebet; den højeste angrebsværdi giver 1 skade, og lige angreb giver ingen skade.
- Hver kamprunde giver 1 erfaring. Spilleren kan træne ved en hokora op til kampfærdighed 6: niveau 3 koster 8 erfaring og 1 skat, niveau 4 koster 10 erfaring og 2 skatte, niveau 5 koster 12 erfaring og 3 skatte, og niveau 6 koster 14 erfaring og 4 skatte.
- Hvis spillerens mindst to terninger viser samme værdi, kan spilleren vælge at flygte gennem døren tilbage til forrige rum. Flugt fjerner den kamperfaring, der er optjent i det aktuelle rum; fra første rum er der ingen flugtvej.
- Et væsen kan flygte gennem nærmeste dør, når dets mindst to terninger viser samme værdi. Væsener med kampfærdighed 1–3 flygter automatisk; væsener med færdighed 4–6 flygter kun, hvis de ikke er stærkere end spilleren.
- Liv genvindes med 1 per tur uden kamp, op til maksimum. En kamptur giver ikke liv tilbage.
- Terningeslagene vises i kampresultatet, og en kamp kan startes, når et synligt væsen er højst to felter væk.

## 11. Præsentation og stemning

- Enkel, læsbar grid-baseret præsentation.
- **人** fungerer glimrende som minimalistisk markør for spilleren.
- Japanske tegn, symboler og små animationer kan understøtte stemningen uden at dominere.
- Fredelige, høflige møder må gerne have små visuelle øjeblikke — fx at give plads og løfte/sænke en paraply.
- Cutscenes er tilladt, men skal understøtte rejsen frem for at afbryde den konstant.

## 12. Åbne spørgsmål — ikke besluttet

- Skal kamp udvides med andre handlinger, forsvar eller timing ud over den nuværende simple terningeudveksling?
- Skal spillet have **kun sværd**, eller også bue?
- Hvordan løses **samtidige bevægelser** teknisk, fx hvis to aggressive aktører vil ind på samme felt?
- Hvordan opdeles **specifik erfaring med et væsen** og mere generel kunnen?
- Hvilke handlinger og observationer giver erfaring, og hvordan undgår vi gentagen farming?
- Hvor meget skal **lyd** og skjul i buske/græs betyde ud over line of sight?
- Skal heling og genvækst afhænge af **spilture, virkelig tid eller begge dele**?
- Skal der være **portable checkpoints**, og i givet fald med hvilke begrænsninger?
- Hvordan opbevares fund sikkert uden for et checkpoint?
- Skal man kunne have **flere samtidige rejser** fra forskellige måneder?
- Hvordan ser en eventuel **rejsedagbog** ud? (Forslag, ikke beslutning.)

## 13. Foreslået første prototype — ikke en bindende beslutning

Byg ét lille rum uden kamp med:

1. En spiller (**人**) og ét væsen.
2. Træer og buske, der påvirker bevægelse og syn.
3. Samtidige ture, inklusive handlingen **vent**.
4. Zone of interest og zone of combat.
5. En udgang, der kan nås ved at observere og undgå væsenet.
6. En lille model for gensidig erfaring og væsenets hukommelse.

**Formål:** Afprøv, om bevægelse, observation og valg er interessante, *før* kampsystemet designes.

---

**Designkompas:** *Mindre action. Mere musha shugyō. En verden, der husker — og to parter, der lærer af hvert møde.*
