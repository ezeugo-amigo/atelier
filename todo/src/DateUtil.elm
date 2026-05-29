module DateUtil exposing
    ( addDays
    , dayOfMonth
    , daysBetween
    , daysInMonth
    , monthIndex
    , monthName
    , prettyDate
    , relativeLabel
    , toISO
    , weekdayIndex
    , weekdayName
    , weekdayShort
    , year
    )

{-| Self-contained date helpers over ISO date strings ("YYYY-MM-DD").

No external time dependency — dates are treated as civil (proleptic
Gregorian) calendar dates and converted to/from a day count using Howard
Hinnant's `days_from_civil` / `civil_from_days` algorithms. "Today" is supplied
by JavaScript through flags, so this module never needs the wall clock.

-}


months : List String
months =
    [ "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December" ]


monthsShort : List String
monthsShort =
    [ "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" ]


weekdays : List String
weekdays =
    [ "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday" ]


weekdaysShort : List String
weekdaysShort =
    [ "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat" ]


pad : Int -> String
pad n =
    String.padLeft 2 '0' (String.fromInt n)


nth : Int -> List String -> String
nth i xs =
    List.drop i xs |> List.head |> Maybe.withDefault ""



-- PARSING / FORMATTING


{-| Parse "YYYY-MM-DD" into a (year, month, day) tuple. Falls back to the
1970 epoch on malformed input so callers never have to thread a Maybe.
-}
parse : String -> ( Int, Int, Int )
parse iso =
    case String.split "-" iso |> List.map (String.toInt >> Maybe.withDefault 0) of
        [ y, m, d ] ->
            ( y, m, d )

        _ ->
            ( 1970, 1, 1 )


toISO : Int -> Int -> Int -> String
toISO y m d =
    String.fromInt y ++ "-" ++ pad m ++ "-" ++ pad d



-- CIVIL <-> DAY COUNT (days since 1970-01-01)


daysFromCivil : Int -> Int -> Int -> Int
daysFromCivil y0 m d =
    let
        y =
            if m <= 2 then
                y0 - 1

            else
                y0

        era =
            (if y >= 0 then
                y

             else
                y - 399
            )
                // 400

        yoe =
            y - era * 400

        mp =
            if m > 2 then
                m - 3

            else
                m + 9

        doy =
            (153 * mp + 2) // 5 + d - 1

        doe =
            yoe * 365 + yoe // 4 - yoe // 100 + doy
    in
    era * 146097 + doe - 719468


civilFromDays : Int -> ( Int, Int, Int )
civilFromDays z0 =
    let
        z =
            z0 + 719468

        era =
            (if z >= 0 then
                z

             else
                z - 146096
            )
                // 146097

        doe =
            z - era * 146097

        yoe =
            (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365

        y =
            yoe + era * 400

        doy =
            doe - (365 * yoe + yoe // 4 - yoe // 100)

        mp =
            (5 * doy + 2) // 153

        d =
            doy - (153 * mp + 2) // 5 + 1

        m =
            if mp < 10 then
                mp + 3

            else
                mp - 9
    in
    ( if m <= 2 then
        y + 1

      else
        y
    , m
    , d
    )


toDayCount : String -> Int
toDayCount iso =
    let
        ( y, m, d ) =
            parse iso
    in
    daysFromCivil y m d



-- PUBLIC API


addDays : Int -> String -> String
addDays n iso =
    let
        ( y, m, d ) =
            civilFromDays (toDayCount iso + n)
    in
    toISO y m d


{-| Number of days from `a` to `b` (positive when `b` is later). -}
daysBetween : String -> String -> Int
daysBetween a b =
    toDayCount b - toDayCount a


{-| 0 = Sunday … 6 = Saturday. 1970-01-01 (day 0) was a Thursday (= 4). -}
weekdayIndex : String -> Int
weekdayIndex iso =
    modBy 7 (toDayCount iso + 4)


weekdayName : String -> String
weekdayName iso =
    nth (weekdayIndex iso) weekdays


weekdayShort : Int -> String
weekdayShort i =
    nth (modBy 7 i) weekdaysShort


monthIndex : String -> Int
monthIndex iso =
    let
        ( _, m, _ ) =
            parse iso
    in
    m - 1


monthName : Int -> String
monthName i =
    nth i months


year : String -> Int
year iso =
    let
        ( y, _, _ ) =
            parse iso
    in
    y


dayOfMonth : String -> Int
dayOfMonth iso =
    let
        ( _, _, d ) =
            parse iso
    in
    d


daysInMonth : Int -> Int -> Int
daysInMonth y m =
    let
        leap =
            (modBy 4 y == 0 && modBy 100 y /= 0) || modBy 400 y == 0
    in
    case m of
        2 ->
            if leap then
                29

            else
                28

        4 ->
            30

        6 ->
            30

        9 ->
            30

        11 ->
            30

        _ ->
            31


prettyDate : String -> String
prettyDate iso =
    let
        ( y, m, d ) =
            parse iso
    in
    nth (m - 1) monthsShort ++ " " ++ String.fromInt d ++ ", " ++ String.fromInt y


{-| Header eyebrow label relative to today. -}
relativeLabel : String -> String -> String
relativeLabel today iso =
    case daysBetween iso today of
        0 ->
            "Today"

        1 ->
            "Yesterday"

        diff ->
            if diff > 1 && diff < 7 then
                String.fromInt diff ++ " days ago"

            else
                prettyDate iso
