module View.Common exposing
    ( formatReceivedAt
    , icon
    , iconForRole
    , onEnter
    , providerDisplayName
    , suggestedEmail
    , viewLabel
    , viewUnreadCount
    )

import Html exposing (Html, span, text)
import Html.Attributes as A
import Html.Events as Ev
import Json.Decode as Decode
import Svg
import Svg.Attributes as SA
import Time
import Types exposing (..)



-- TIME FORMATTING


{-| Relative for today and yesterday, absolute beyond. The wire carries an
instant, so this is the only place a timestamp becomes prose.

`now` is the reference instant. Passing it in rather than reading a clock keeps
the view a pure function.

-}
formatReceivedAt : Time.Zone -> Time.Posix -> Time.Posix -> String
formatReceivedAt zone now received =
    let
        sameDay a b =
            Time.toYear zone a == Time.toYear zone b
                && Time.toMonth zone a == Time.toMonth zone b
                && Time.toDay zone a == Time.toDay zone b

        yesterday =
            Time.millisToPosix (Time.posixToMillis now - 86400000)

        clock =
            let
                hour24 =
                    Time.toHour zone received

                hour12 =
                    if modBy 12 hour24 == 0 then
                        12

                    else
                        modBy 12 hour24
            in
            String.fromInt hour12
                ++ ":"
                ++ String.padLeft 2 '0' (String.fromInt (Time.toMinute zone received))
                ++ (if hour24 < 12 then
                        " AM"

                    else
                        " PM"
                   )
    in
    if sameDay received now then
        clock

    else if sameDay received yesterday then
        "Yesterday"

    else if Time.toYear zone received == Time.toYear zone now then
        monthName (Time.toMonth zone received) ++ " " ++ String.fromInt (Time.toDay zone received)

    else
        monthName (Time.toMonth zone received)
            ++ " "
            ++ String.fromInt (Time.toDay zone received)
            ++ ", "
            ++ String.fromInt (Time.toYear zone received)


monthName : Time.Month -> String
monthName month =
    case month of
        Time.Jan ->
            "Jan"

        Time.Feb ->
            "Feb"

        Time.Mar ->
            "Mar"

        Time.Apr ->
            "Apr"

        Time.May ->
            "May"

        Time.Jun ->
            "Jun"

        Time.Jul ->
            "Jul"

        Time.Aug ->
            "Aug"

        Time.Sep ->
            "Sep"

        Time.Oct ->
            "Oct"

        Time.Nov ->
            "Nov"

        Time.Dec ->
            "Dec"



-- SHARED VIEWS


viewLabel : String -> Html msg
viewLabel value =
    span [ A.class ("chip " ++ labelClass value) ] [ text value ]


labelClass : String -> String
labelClass value =
    case String.toLower value of
        "work" ->
            "work"

        "finance" ->
            "finance"

        _ ->
            ""


viewUnreadCount : Int -> Html msg
viewUnreadCount unreadCount =
    if unreadCount > 0 then
        span [ A.class "count" ] [ text (String.fromInt unreadCount) ]

    else
        span [] []


providerDisplayName : Model -> String -> String
providerDisplayName model provider =
    model.providerOptions
        |> List.filter (\option -> option.provider == provider)
        |> List.head
        |> Maybe.map .displayName
        |> Maybe.withDefault
            (case provider of
                "gmail" ->
                    "Gmail"

                "mockGmail" ->
                    "Mock Gmail"

                "mockOutlook" ->
                    "Mock Outlook"

                _ ->
                    "Provider"
            )


suggestedEmail : String -> String
suggestedEmail provider =
    case provider of
        "mockGmail" ->
            "you@gmail.test"

        "mockOutlook" ->
            "you@outlook.test"

        _ ->
            "you@example.test"


onEnter : msg -> Html.Attribute msg
onEnter msg =
    Ev.on "keydown"
        (Decode.field "key" Decode.string
            |> Decode.andThen
                (\key ->
                    if key == "Enter" then
                        Decode.succeed msg

                    else
                        Decode.fail "not enter"
                )
        )



-- ICONS


iconForRole : String -> Html msg
iconForRole role =
    case role of
        "inbox" ->
            icon "inbox"

        "starred" ->
            icon "star"

        "drafts" ->
            icon "edit"

        "sent" ->
            icon "send"

        "archive" ->
            icon "archive"

        "trash" ->
            icon "trash"

        "spam" ->
            icon "alert"

        _ ->
            icon "folder"


icon : String -> Html msg
icon name =
    let
        attrs =
            [ SA.class "icon"
            , SA.viewBox "0 0 24 24"
            , SA.fill "none"
            , SA.stroke "currentColor"
            , SA.strokeWidth "2"
            , SA.strokeLinecap "round"
            , SA.strokeLinejoin "round"
            ]
    in
    case name of
        "alert" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M12 3 2 20h20L12 3Z" ] []
                , Svg.path [ SA.d "M12 9v5" ] []
                , Svg.path [ SA.d "M12 17h.01" ] []
                ]

        "archive" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M21 8v13H3V8" ] []
                , Svg.path [ SA.d "M1 3h22v5H1z" ] []
                , Svg.path [ SA.d "M10 12h4" ] []
                ]

        "arrow-right" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M5 12h14" ] []
                , Svg.path [ SA.d "m12 5 7 7-7 7" ] []
                ]

        "arrow-left" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M19 12H5" ] []
                , Svg.path [ SA.d "m12 19-7-7 7-7" ] []
                ]

        "check" ->
            Svg.svg attrs [ Svg.path [ SA.d "M20 6 9 17l-5-5" ] [] ]

        "chevron-down" ->
            Svg.svg attrs [ Svg.path [ SA.d "m6 9 6 6 6-6" ] [] ]

        "chevron-right" ->
            Svg.svg attrs [ Svg.path [ SA.d "m9 6 6 6-6 6" ] [] ]

        "dot" ->
            Svg.svg attrs
                [ Svg.circle
                    [ SA.cx "12", SA.cy "12", SA.r "4", SA.fill "currentColor", SA.stroke "none" ]
                    []
                ]

        "edit" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M12 20h9" ] []
                , Svg.path [ SA.d "M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" ] []
                ]

        "folder" ->
            Svg.svg attrs [ Svg.path [ SA.d "M3 7h6l2 2h10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" ] [] ]

        "globe" ->
            Svg.svg attrs
                [ Svg.circle [ SA.cx "12", SA.cy "12", SA.r "9" ] []
                , Svg.path [ SA.d "M3 12h18" ] []
                , Svg.path [ SA.d "M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18Z" ] []
                ]

        "inbox" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M22 12h-6l-2 3h-4l-2-3H2" ] []
                , Svg.path [ SA.d "m5.45 5.11-3.1 6.2A2 2 0 0 0 2.24 13L4 19a2 2 0 0 0 2 1h12a2 2 0 0 0 2-1l1.76-6a2 2 0 0 0-.11-1.69l-3.1-6.2A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z" ] []
                ]

        "key" ->
            Svg.svg attrs
                [ Svg.circle [ SA.cx "7.5", SA.cy "14.5", SA.r "3.5" ] []
                , Svg.path [ SA.d "M10 12 21 1" ] []
                , Svg.path [ SA.d "m14 8 3 3" ] []
                , Svg.path [ SA.d "m17 5 3 3" ] []
                ]

        "link-out" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M14 4h6v6" ] []
                , Svg.path [ SA.d "M20 4 10 14" ] []
                , Svg.path [ SA.d "M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" ] []
                ]

        "mail" ->
            Svg.svg attrs
                [ Svg.rect [ SA.x "3", SA.y "5", SA.width "18", SA.height "14", SA.rx "2" ] []
                , Svg.path [ SA.d "m3 7 9 6 9-6" ] []
                ]

        "plus" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M12 5v14" ] []
                , Svg.path [ SA.d "M5 12h14" ] []
                ]

        "refresh" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M21 12a9 9 0 0 0-15.5-6.2L3 8" ] []
                , Svg.path [ SA.d "M3 3v5h5" ] []
                , Svg.path [ SA.d "M3 12a9 9 0 0 0 15.5 6.2L21 16" ] []
                , Svg.path [ SA.d "M16 16h5v5" ] []
                ]

        "search" ->
            Svg.svg attrs
                [ Svg.circle [ SA.cx "11", SA.cy "11", SA.r "7" ] []
                , Svg.path [ SA.d "m21 21-4.3-4.3" ] []
                ]

        "send" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "m22 2-7 20-4-9-9-4Z" ] []
                , Svg.path [ SA.d "M22 2 11 13" ] []
                ]

        "star" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "m12 2 3.1 6.4 6.9 1-5 4.9 1.2 6.8L12 17.8 5.8 21.1 7 14.3 2 9.4l6.9-1Z" ] [] ]

        "trash" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M3 6h18" ] []
                , Svg.path [ SA.d "M8 6V4h8v2" ] []
                , Svg.path [ SA.d "m6 6 1 15h10l1-15" ] []
                ]

        "unlink" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M8 12H5a3 3 0 0 1 0-6h3" ] []
                , Svg.path [ SA.d "M16 6h3a3 3 0 0 1 0 6h-3" ] []
                , Svg.path [ SA.d "M4 20 20 4" ] []
                ]

        "x" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M18 6 6 18" ] []
                , Svg.path [ SA.d "m6 6 12 12" ] []
                ]

        _ ->
            Svg.svg attrs [ Svg.circle [ SA.cx "12", SA.cy "12", SA.r "8" ] [] ]
