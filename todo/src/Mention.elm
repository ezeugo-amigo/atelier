module Mention exposing
    ( Segment(..)
    , Token
    , activeToken
    , handles
    , insert
    , segments
    , suggestions
    )

{-| @mention parsing for task text.

Mentions are stored as plain text (`@ada`) — there is no separate people table
and no id substitution. Everything here is a pure reading of a string, which
keeps persistence unchanged and makes a mention survive any edit that leaves
the `@handle` intact.

`segments` splits a string for rendering (the chip highlighter), `activeToken`
answers "is the caret inside a mention right now?" for the suggestion popup,
and `suggestions` harvests the people you have mentioned before.

-}

import Dict


{-| A run of a task's text: either ordinary text or a mention to draw as a chip.
The concatenation of every segment's source text is the original string, so
character offsets are preserved (the caret measurement in mentions.js relies on
that).
-}
type Segment
    = Plain String
    | Chip String


{-| The mention the caret currently sits in: where its `@` starts, and the
handle text typed so far.
-}
type alias Token =
    { start : Int
    , query : String
    }



-- CHARACTER CLASSES


{-| Characters that may appear inside a handle. `.` and `-` are allowed so
`@ada.lovelace` and `@jean-luc` work, but see `stripTrailing` — a handle can't
end on one, otherwise "ask @ada." would swallow the full stop.
-}
isHandleChar : Char -> Bool
isHandleChar c =
    Char.isAlphaNum c || c == '_' || c == '-' || c == '.'


{-| An `@` only opens a mention at the start of the text or after a break, so
that email addresses (`ada@example.com`) stay plain text.
-}
opensMention : Maybe Char -> Bool
opensMention prev =
    case prev of
        Nothing ->
            True

        Just c ->
            not (Char.isAlphaNum c || c == '_' || c == '@')


stripTrailing : List Char -> List Char
stripTrailing chars =
    chars
        |> List.reverse
        |> dropWhile (\c -> c == '.' || c == '-' || c == '_')
        |> List.reverse


takeWhile : (a -> Bool) -> List a -> List a
takeWhile pred list =
    case list of
        x :: rest ->
            if pred x then
                x :: takeWhile pred rest

            else
                []

        [] ->
            []


dropWhile : (a -> Bool) -> List a -> List a
dropWhile pred list =
    case list of
        x :: rest ->
            if pred x then
                dropWhile pred rest

            else
                list

        [] ->
            []



-- SEGMENTS


segments : String -> List Segment
segments text =
    parse (String.toList text) Nothing [] []


parse : List Char -> Maybe Char -> List Char -> List Segment -> List Segment
parse chars prev plain acc =
    case chars of
        [] ->
            List.reverse (flush plain acc)

        c :: rest ->
            if c == '@' && opensMention prev then
                let
                    ( handle, remaining ) =
                        takeHandle rest
                in
                if handle == "" then
                    parse rest (Just c) (c :: plain) acc

                else
                    parse remaining
                        (String.toList handle |> List.reverse |> List.head)
                        []
                        (Chip handle :: flush plain acc)

            else
                parse rest (Just c) (c :: plain) acc


{-| Read the handle following an `@`, handing back any trailing punctuation so
the caller keeps scanning it as ordinary text.
-}
takeHandle : List Char -> ( String, List Char )
takeHandle chars =
    let
        run =
            takeWhile isHandleChar chars

        rest =
            List.drop (List.length run) chars

        kept =
            stripTrailing run

        pushedBack =
            List.drop (List.length kept) run
    in
    ( String.fromList kept, pushedBack ++ rest )


flush : List Char -> List Segment -> List Segment
flush plain acc =
    if List.isEmpty plain then
        acc

    else
        Plain (String.fromList (List.reverse plain)) :: acc


handles : String -> List String
handles text =
    segments text
        |> List.filterMap
            (\segment ->
                case segment of
                    Chip handle ->
                        Just handle

                    Plain _ ->
                        Nothing
            )



-- CARET


{-| Is the caret inside a mention being typed? Scans backwards from the caret
over handle characters and expects an opening `@`.
-}
activeToken : String -> Int -> Maybe Token
activeToken text caret =
    let
        beforeCaret =
            String.toList text |> List.take caret |> List.reverse

        queryReversed =
            takeWhile isHandleChar beforeCaret
    in
    case List.drop (List.length queryReversed) beforeCaret of
        '@' :: before ->
            if opensMention (List.head before) then
                Just
                    { start = caret - List.length queryReversed - 1
                    , query = String.fromList (List.reverse queryReversed)
                    }

            else
                Nothing

        _ ->
            Nothing


{-| Replace the partially typed mention between `start` and `caret` with the
chosen handle, and report where the caret should land afterwards.

A mention is followed by a space so you can keep typing. When the text already
has one there, we reuse it and step the caret over it rather than adding a
second.
-}
insert : String -> Int -> Int -> String -> ( String, Int )
insert text start caret handle =
    let
        tail =
            String.dropLeft caret text

        spaceFollows =
            String.left 1 tail == " "

        replacement =
            if spaceFollows then
                "@" ++ handle

            else
                "@" ++ handle ++ " "
    in
    ( String.left start text ++ replacement ++ tail
    , start
        + String.length replacement
        + (if spaceFollows then
            1

           else
            0
          )
    )



-- SUGGESTIONS


{-| People mentioned before, most-used first, filtered by what's been typed.
Matching is case-insensitive but the spelling you used first is what's offered.
-}
suggestions : List String -> String -> List String
suggestions texts query =
    texts
        |> List.concatMap handles
        |> List.foldl
            (\handle counts ->
                Dict.update (String.toLower handle)
                    (\entry ->
                        case entry of
                            Just ( display, n ) ->
                                Just ( display, n + 1 )

                            Nothing ->
                                Just ( handle, 1 )
                    )
                    counts
            )
            Dict.empty
        |> Dict.toList
        |> List.filter (\( key, _ ) -> String.startsWith (String.toLower query) key)
        |> List.sortWith
            (\( keyA, ( _, countA ) ) ( keyB, ( _, countB ) ) ->
                compare ( -countA, keyA ) ( -countB, keyB )
            )
        |> List.map (\( _, ( display, _ ) ) -> display)
        |> List.take 6
