port module Main exposing (main)

{-| Today — a warm, paper-like todo list.

A faithful re-implementation of the Claude Design "Today.html" prototype in Elm.
Open tasks carry over to the current day automatically; a calendar view shows
completion history. State persists to IndexedDB through ports (see db.js).

The look is fixed to the direction the design landed on — the Paper surface with
the Clay accent, serif weekday, regular density — so the prototype's exploratory
"Tweaks" panel is intentionally omitted.

-}

import Browser
import DateUtil
import Dict exposing (Dict)
import Html exposing (Html, button, div, h1, header, p, section, span, text, textarea)
import Html.Attributes as A
import Html.Events as Ev
import Json.Decode as Decode
import Json.Encode as Encode
import Mention
import Svg
import Svg.Attributes as SA



-- PORTS


{-| Ask db.js to read the persisted task list. -}
port dbLoad : () -> Cmd msg


{-| Hand db.js the full task list to persist. -}
port dbSave : Encode.Value -> Cmd msg


{-| db.js delivers `{ found : Bool, tasks : [...] }` here. -}
port dbLoaded : (Encode.Value -> msg) -> Sub msg


{-| JavaScript tells Elm when the local civil date changes while the app is
already running. Elm still owns all task/calendar state; JS only supplies the
wall-clock date string.
-}
port todayChanged : (String -> msg) -> Sub msg


port taskDragStarted : (String -> msg) -> Sub msg


port taskDragOver : (String -> msg) -> Sub msg


port taskDragOverAfter : (String -> msg) -> Sub msg


port taskDropped : (String -> msg) -> Sub msg


port taskDroppedAfter : (String -> msg) -> Sub msg


port taskDragEnded : (() -> msg) -> Sub msg


{-| Ask mentions.js where a character sits on screen, so the suggestion popup
can be pinned under the `@` the caret is in. Elm can't measure text.
-}
port caretQuery : Encode.Value -> Cmd msg


{-| mentions.js answers with a `CaretPos` in viewport coordinates. -}
port caretPos : (Encode.Value -> msg) -> Sub msg


{-| Put the caret back after Elm rewrites a field's text to accept a mention. -}
port setCaret : Encode.Value -> Cmd msg



-- MODEL


type alias Task =
    { id : String
    , title : String
    , note : String
    , done : Bool
    , createdAt : String -- ISO date the task was first created
    , completedAt : Maybe String -- ISO date it was checked off
    , day : String -- ISO date it currently lives on (carry-over moves this)
    , order : Int
    }


type ViewMode
    = TodayView
    | CalendarView


{-| Which editable field the mention popup belongs to. Every field renders with
a stable DOM id (see `fieldId`) so mentions.js can find it. -}
type MentionField
    = AddField
    | TitleField String
    | NoteField String


{-| Where the `@` sits, as measured by mentions.js, with the window size it was
measured against so the popup can be kept on screen.
-}
type alias CaretPos =
    { fieldId : String
    , x : Float
    , y : Float
    , lineTop : Float
    , viewWidth : Float
    , viewHeight : Float
    }


{-| An open suggestion popup. `start` is the offset of the `@`, `caret` where
the caret was when we last looked, and `pos` the measurement (Nothing until
mentions.js answers, which keeps the popup from flashing at the top-left
corner).
-}
type alias MentionMenu =
    { field : MentionField
    , start : Int
    , caret : Int
    , query : String
    , index : Int
    , pos : Maybe CaretPos
    }


type alias Model =
    { tasks : List Task
    , view : ViewMode
    , today : String
    , addInput : String
    , openId : Maybe String -- task whose note drawer is expanded
    , calCursor : ( Int, Int ) -- (year, monthIndex 0..11) shown in the calendar
    , calSelected : String -- ISO date selected for the day-detail panel
    , nextId : Int
    , loaded : Bool
    , draggingId : Maybe String
    , dragOverId : Maybe String
    , dropAfterId : Maybe String
    , mention : Maybe MentionMenu
    }


type alias Flags =
    { today : String
    , seed : Int
    }


init : Flags -> ( Model, Cmd Msg )
init flags =
    ( { tasks = []
      , view = TodayView
      , today = flags.today
      , addInput = ""
      , openId = Nothing
      , calCursor = ( DateUtil.year flags.today, DateUtil.monthIndex flags.today )
      , calSelected = flags.today
      , nextId = flags.seed
      , loaded = False
      , draggingId = Nothing
      , dragOverId = Nothing
      , dropAfterId = Nothing
      , mention = Nothing
      }
    , dbLoad ()
    )



-- UPDATE


type Msg
    = GotStored Encode.Value
    | GotToday String
    | SetView ViewMode
    | FieldInput MentionField String Int
    | FieldCaret MentionField String Int
    | FieldEnter MentionField String Int
    | SubmitAdd
    | Toggle String
    | ToggleOpen String
    | GotCaretPos Encode.Value
    | MentionMove Int
    | MentionHover Int
    | MentionPick String
    | MentionAccept
    | MentionClose
    | DragStart String
    | DragOver String
    | DragOverAfter String
    | Drop String
    | DropAfter String
    | DragEnd
    | Delete String
    | CalShift Int
    | CalSelect String


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        GotStored value ->
            let
                stored =
                    case Decode.decodeValue storedDecoder value of
                        Ok s ->
                            if s.found then
                                s.tasks

                            else
                                []

                        Err _ ->
                            []

                carried =
                    applyCarryOver model.today stored
            in
            ( { model | tasks = carried, loaded = True }
            , dbSave (encodeTasks carried)
            )

        GotToday newToday ->
            moveToToday newToday model

        SetView v ->
            ( { model | view = v }, Cmd.none )

        FieldInput field value caret ->
            let
                edited =
                    setFieldText field value model

                menu =
                    recomputeMenu field value caret edited
            in
            ( { edited | mention = menu }
            , Cmd.batch [ saveField field edited, askCaret menu ]
            )

        FieldCaret field value caret ->
            let
                menu =
                    recomputeMenu field value caret model
            in
            ( { model | mention = menu }, askCaret menu )

        FieldEnter field value caret ->
            -- What Enter means depends on whether a mention is being typed, and
            -- a view-time answer can be a frame stale when you type fast. Decide
            -- here instead, against the value the DOM just reported.
            let
                edited =
                    setFieldText field value model

                menu =
                    recomputeMenu field value caret edited

                chosen =
                    menu
                        |> Maybe.andThen
                            (\m ->
                                if m.pos == Nothing then
                                    -- The popup hasn't been measured yet, so it
                                    -- isn't on screen to accept from.
                                    Nothing

                                else
                                    List.drop m.index (menuItems edited m)
                                        |> List.head
                                        |> Maybe.andThen
                                            (\handle ->
                                                -- Typing the whole name is its
                                                -- own way of choosing it. Taking
                                                -- Enter here would swallow the
                                                -- keystroke for no visible gain.
                                                if String.toLower handle == String.toLower m.query then
                                                    Nothing

                                                else
                                                    Just ( m, handle )
                                            )
                            )
            in
            case chosen of
                Just ( m, handle ) ->
                    acceptMention m handle edited

                Nothing ->
                    let
                        settled =
                            { edited | mention = menu }
                    in
                    case field of
                        AddField ->
                            submitAdd settled

                        TitleField _ ->
                            -- Titles stay one line, wrapped.
                            ( settled, saveField field settled )

                        NoteField _ ->
                            -- The keydown was cancelled to keep Enter available
                            -- for the popup, so the newline is ours to insert.
                            insertNewline field value caret settled

        GotCaretPos value ->
            case ( Decode.decodeValue caretPosDecoder value, model.mention ) of
                ( Ok pos, Just menu ) ->
                    if pos.fieldId == fieldId menu.field then
                        ( { model | mention = Just { menu | pos = Just pos } }, Cmd.none )

                    else
                        ( model, Cmd.none )

                _ ->
                    ( model, Cmd.none )

        MentionMove delta ->
            case model.mention of
                Just menu ->
                    let
                        count =
                            List.length (menuItems model menu)
                    in
                    if count == 0 then
                        ( model, Cmd.none )

                    else
                        ( { model | mention = Just { menu | index = modBy count (menu.index + delta) } }
                        , Cmd.none
                        )

                Nothing ->
                    ( model, Cmd.none )

        MentionHover index ->
            case model.mention of
                Just menu ->
                    ( { model | mention = Just { menu | index = index } }, Cmd.none )

                Nothing ->
                    ( model, Cmd.none )

        MentionPick handle ->
            case model.mention of
                Just menu ->
                    acceptMention menu handle model

                Nothing ->
                    ( model, Cmd.none )

        MentionAccept ->
            case model.mention of
                Just menu ->
                    case List.drop menu.index (menuItems model menu) |> List.head of
                        Just handle ->
                            acceptMention menu handle model

                        Nothing ->
                            ( model, Cmd.none )

                Nothing ->
                    ( model, Cmd.none )

        MentionClose ->
            ( { model | mention = Nothing }, Cmd.none )

        SubmitAdd ->
            submitAdd model

        Toggle id ->
            persist (mapTask id (toggleTask model.today) model)

        ToggleOpen id ->
            ( { model
                | openId =
                    if model.openId == Just id then
                        Nothing

                    else
                        Just id
              }
            , Cmd.none
            )

        DragStart id ->
            ( { model | draggingId = Just id, dragOverId = Nothing, dropAfterId = Nothing }, Cmd.none )

        DragOver id ->
            ( { model
                | dragOverId =
                    if model.draggingId == Just id || isImmediateSuccessor model.today model.draggingId id model then
                        Nothing

                    else
                        Just id
                , dropAfterId = Nothing
              }
            , Cmd.none
            )

        DragOverAfter id ->
            ( { model | dragOverId = Nothing, dropAfterId = Just id }, Cmd.none )

        Drop targetId ->
            case model.draggingId of
                Just draggedId ->
                    let
                        reordered =
                            reorderTaskBefore model.today draggedId targetId model
                    in
                    ( { reordered | draggingId = Nothing, dragOverId = Nothing, dropAfterId = Nothing }
                    , if reordered.tasks == model.tasks then
                        Cmd.none

                      else
                        dbSave (encodeTasks reordered.tasks)
                    )

                Nothing ->
                    ( model, Cmd.none )

        DropAfter anchorId ->
            case model.draggingId of
                Just draggedId ->
                    let
                        reordered =
                            reorderTaskAfter model.today draggedId anchorId model
                    in
                    ( { reordered | draggingId = Nothing, dragOverId = Nothing, dropAfterId = Nothing }
                    , if reordered.tasks == model.tasks then
                        Cmd.none

                      else
                        dbSave (encodeTasks reordered.tasks)
                    )

                Nothing ->
                    ( model, Cmd.none )

        DragEnd ->
            ( { model | draggingId = Nothing, dragOverId = Nothing, dropAfterId = Nothing }, Cmd.none )

        Delete id ->
            persist { model | tasks = List.filter (\t -> t.id /= id) model.tasks }

        CalShift delta ->
            let
                ( y, m ) =
                    model.calCursor

                total =
                    y * 12 + m + delta
            in
            ( { model | calCursor = ( total // 12, modBy 12 total ) }, Cmd.none )

        CalSelect iso ->
            ( { model | calSelected = iso }, Cmd.none )


moveToToday : String -> Model -> ( Model, Cmd Msg )
moveToToday newToday model =
    if newToday == model.today then
        ( model, Cmd.none )

    else
        let
            oldToday =
                model.today

            oldTodayCursor =
                ( DateUtil.year oldToday, DateUtil.monthIndex oldToday )

            newTodayCursor =
                ( DateUtil.year newToday, DateUtil.monthIndex newToday )

            movedTasks =
                if newToday > oldToday then
                    applyCarryOver newToday model.tasks

                else
                    model.tasks

            nextModel =
                { model
                    | today = newToday
                    , tasks = movedTasks
                    , calSelected =
                        if model.calSelected == oldToday then
                            newToday

                        else
                            model.calSelected
                    , calCursor =
                        if model.calCursor == oldTodayCursor then
                            newTodayCursor

                        else
                            model.calCursor
                }

            saveCmd =
                if model.loaded && newToday > oldToday then
                    dbSave (encodeTasks movedTasks)

                else
                    Cmd.none
        in
        ( nextModel, saveCmd )


submitAdd : Model -> ( Model, Cmd Msg )
submitAdd model =
    let
        title =
            String.trim model.addInput
    in
    if title == "" then
        ( model, Cmd.none )

    else
        let
            maxOrder =
                model.tasks
                    |> List.filter (\t -> t.day == model.today)
                    |> List.map .order
                    |> List.maximum
                    |> Maybe.withDefault 0

            new =
                { id = "t" ++ String.fromInt model.nextId
                , title = title
                , note = ""
                , done = False
                , createdAt = model.today
                , completedAt = Nothing
                , day = model.today
                , order = maxOrder + 1
                }
        in
        persist
            { model
                | tasks = model.tasks ++ [ new ]
                , addInput = ""
                , nextId = model.nextId + 1
                , mention = Nothing
            }


persist : Model -> ( Model, Cmd Msg )
persist model =
    ( model, dbSave (encodeTasks model.tasks) )



-- MENTIONS


fieldId : MentionField -> String
fieldId field =
    case field of
        AddField ->
            "mfield-add"

        TitleField id ->
            "mfield-title-" ++ id

        NoteField id ->
            "mfield-note-" ++ id


fieldText : MentionField -> Model -> String
fieldText field model =
    case field of
        AddField ->
            model.addInput

        TitleField id ->
            taskById id model |> Maybe.map .title |> Maybe.withDefault ""

        NoteField id ->
            taskById id model |> Maybe.map .note |> Maybe.withDefault ""


setFieldText : MentionField -> String -> Model -> Model
setFieldText field value model =
    case field of
        AddField ->
            { model | addInput = value }

        TitleField id ->
            mapTask id (\t -> { t | title = value }) model

        NoteField id ->
            mapTask id (\t -> { t | note = value }) model


{-| The add row isn't a task yet, so it has nothing to persist. -}
saveField : MentionField -> Model -> Cmd Msg
saveField field model =
    case field of
        AddField ->
            Cmd.none

        _ ->
            dbSave (encodeTasks model.tasks)


taskById : String -> Model -> Maybe Task
taskById id model =
    List.filter (\t -> t.id == id) model.tasks |> List.head


{-| Everyone mentioned anywhere in the list, minus the half-typed mention the
caret is sitting in — offering you back the fragment you're typing is no help.
-}
mentionSuggestions : MentionField -> Int -> String -> Model -> List String
mentionSuggestions field start query model =
    let
        mask text =
            String.left start text ++ String.dropLeft (start + 1 + String.length query) text

        textsOf task =
            [ if field == TitleField task.id then
                mask task.title

              else
                task.title
            , if field == NoteField task.id then
                mask task.note

              else
                task.note
            ]
    in
    Mention.suggestions (List.concatMap textsOf model.tasks) query


menuItems : Model -> MentionMenu -> List String
menuItems model menu =
    mentionSuggestions menu.field menu.start menu.query model


{-| Decide whether the popup should be open after a keystroke or caret move.
Staying on the same `@` keeps the highlighted row and the measured position, so
narrowing the list doesn't reset the selection or make the popup jump.
-}
recomputeMenu : MentionField -> String -> Int -> Model -> Maybe MentionMenu
recomputeMenu field value caret model =
    case Mention.activeToken value caret of
        Nothing ->
            Nothing

        Just token ->
            let
                items =
                    mentionSuggestions field token.start token.query model

                previous =
                    case model.mention of
                        Just menu ->
                            if menu.field == field && menu.start == token.start then
                                Just menu

                            else
                                Nothing

                        Nothing ->
                            Nothing
            in
            if List.isEmpty items then
                Nothing

            else
                Just
                    { field = field
                    , start = token.start
                    , caret = caret
                    , query = token.query
                    , index =
                        previous
                            |> Maybe.map (\menu -> min menu.index (List.length items - 1))
                            |> Maybe.withDefault 0
                    , pos = Maybe.andThen .pos previous
                    }


{-| Anchor the popup to the `@` rather than the caret, so it holds still while
you type the name. -}
askCaret : Maybe MentionMenu -> Cmd Msg
askCaret menu =
    case menu of
        Just m ->
            caretQuery
                (Encode.object
                    [ ( "fieldId", Encode.string (fieldId m.field) )
                    , ( "index", Encode.int m.start )
                    ]
                )

        Nothing ->
            Cmd.none


insertNewline : MentionField -> String -> Int -> Model -> ( Model, Cmd Msg )
insertNewline field value caret model =
    let
        edited =
            setFieldText field (String.left caret value ++ "\n" ++ String.dropLeft caret value) model
    in
    ( { edited | mention = Nothing }
    , Cmd.batch
        [ saveField field edited
        , setCaret
            (Encode.object
                [ ( "fieldId", Encode.string (fieldId field) )
                , ( "index", Encode.int (caret + 1) )
                ]
            )
        ]
    )


acceptMention : MentionMenu -> String -> Model -> ( Model, Cmd Msg )
acceptMention menu handle model =
    let
        ( value, caret ) =
            Mention.insert (fieldText menu.field model) menu.start menu.caret handle

        edited =
            setFieldText menu.field value model
    in
    ( { edited | mention = Nothing }
    , Cmd.batch
        [ saveField menu.field edited
        , setCaret
            (Encode.object
                [ ( "fieldId", Encode.string (fieldId menu.field) )
                , ( "index", Encode.int caret )
                ]
            )
        ]
    )


mapTask : String -> (Task -> Task) -> Model -> Model
mapTask id f model =
    { model
        | tasks =
            List.map
                (\t ->
                    if t.id == id then
                        f t

                    else
                        t
                )
                model.tasks
    }


reorderTaskBefore : String -> String -> String -> Model -> Model
reorderTaskBefore today draggedId targetId model =
    let
        dragged =
            List.filter (\task -> task.id == draggedId) model.tasks |> List.head

        target =
            List.filter (\task -> task.id == targetId) model.tasks |> List.head
    in
    case ( dragged, target ) of
        ( Just draggedTask, Just targetTask ) ->
            if draggedId == targetId || not (sameReorderGroup today draggedTask targetTask) then
                model

            else
                let
                    group =
                        model.tasks
                            |> List.filter (sameReorderGroup today draggedTask)
                            |> List.sortBy .order

                    moved =
                        group
                            |> List.filter (\task -> task.id /= draggedId)
                            |> insertBefore targetId draggedTask

                    renumbered =
                        moved
                            |> List.indexedMap (\index task -> { task | order = index + 1 })

                    orderById =
                        Dict.fromList (List.map (\task -> ( task.id, task.order )) renumbered)
                in
                { model | tasks = applyOrders orderById model.tasks }

        _ ->
            model


sameReorderGroup : String -> Task -> Task -> Bool
sameReorderGroup today first second =
    first.day == today
        && second.day == today
        && not first.done
        && not second.done
        && (first.createdAt < today) == (second.createdAt < today)


isImmediateSuccessor : String -> Maybe String -> String -> Model -> Bool
isImmediateSuccessor today draggingId targetId model =
    case draggingId of
        Just draggedId ->
            let
                dragged =
                    List.filter (\task -> task.id == draggedId) model.tasks |> List.head

                orderedGroup =
                    case dragged of
                        Just draggedTask ->
                            model.tasks
                                |> List.filter (sameReorderGroup today draggedTask)
                                |> List.sortBy .order

                        Nothing ->
                            []
            in
            hasAdjacentPair draggedId targetId orderedGroup

        Nothing ->
            False


hasAdjacentPair : String -> String -> List Task -> Bool
hasAdjacentPair draggedId targetId tasks =
    case tasks of
        first :: second :: rest ->
            (first.id == draggedId && second.id == targetId)
                || hasAdjacentPair draggedId targetId (second :: rest)

        _ ->
            False


applyOrders : Dict String Int -> List Task -> List Task
applyOrders orderById tasks =
    List.map
        (\task ->
            case Dict.get task.id orderById of
                Just order ->
                    { task | order = order }

                Nothing ->
                    task
        )
        tasks


insertBefore : String -> Task -> List Task -> List Task
insertBefore targetId task tasks =
    case tasks of
        first :: rest ->
            if first.id == targetId then
                task :: first :: rest

            else
                first :: insertBefore targetId task rest

        [] ->
            [ task ]


reorderTaskAfter : String -> String -> String -> Model -> Model
reorderTaskAfter today draggedId anchorId model =
    let
        dragged =
            List.filter (\task -> task.id == draggedId) model.tasks |> List.head

        anchor =
            List.filter (\task -> task.id == anchorId) model.tasks |> List.head
    in
    case ( dragged, anchor ) of
        ( Just draggedTask, Just anchorTask ) ->
            if draggedId == anchorId || not (sameReorderGroup today draggedTask anchorTask) then
                model

            else
                let
                    group =
                        model.tasks
                            |> List.filter (sameReorderGroup today draggedTask)
                            |> List.sortBy .order

                    moved =
                        group
                            |> List.filter (\task -> task.id /= draggedId)
                            |> insertAfter anchorId draggedTask

                    renumbered =
                        moved
                            |> List.indexedMap (\index task -> { task | order = index + 1 })

                    orderById =
                        Dict.fromList (List.map (\task -> ( task.id, task.order )) renumbered)
                in
                { model | tasks = applyOrders orderById model.tasks }

        _ ->
            model


insertAfter : String -> Task -> List Task -> List Task
insertAfter anchorId task tasks =
    case tasks of
        first :: rest ->
            if first.id == anchorId then
                first :: task :: rest

            else
                first :: insertAfter anchorId task rest

        [] ->
            [ task ]


toggleTask : String -> Task -> Task
toggleTask today t =
    let
        done =
            not t.done
    in
    { t
        | done = done
        , completedAt =
            if done then
                Just today

            else
                Nothing

        -- Checking off anchors the task to today so history stays truthful.
        , day =
            if done then
                today

            else
                t.day
    }


{-| Any open task whose day is in the past rolls forward to today. Completed
tasks stay anchored to the day they were finished. -}
applyCarryOver : String -> List Task -> List Task
applyCarryOver today tasks =
    List.map
        (\t ->
            if not t.done && t.day < today then
                { t | day = today }

            else
                t
        )
        tasks



-- JSON


storedDecoder : Decode.Decoder { found : Bool, tasks : List Task }
storedDecoder =
    Decode.map2 (\found tasks -> { found = found, tasks = tasks })
        (Decode.field "found" Decode.bool)
        (Decode.field "tasks" (Decode.list taskDecoder))


caretPosDecoder : Decode.Decoder CaretPos
caretPosDecoder =
    Decode.map6 CaretPos
        (Decode.field "fieldId" Decode.string)
        (Decode.field "x" Decode.float)
        (Decode.field "y" Decode.float)
        (Decode.field "lineTop" Decode.float)
        (Decode.field "viewWidth" Decode.float)
        (Decode.field "viewHeight" Decode.float)


taskDecoder : Decode.Decoder Task
taskDecoder =
    Decode.map8 Task
        (Decode.field "id" Decode.string)
        (Decode.field "title" Decode.string)
        (Decode.field "note" Decode.string)
        (Decode.field "done" Decode.bool)
        (Decode.field "createdAt" Decode.string)
        (Decode.field "completedAt" (Decode.nullable Decode.string))
        (Decode.field "day" Decode.string)
        (Decode.field "order" Decode.int)


encodeTasks : List Task -> Encode.Value
encodeTasks tasks =
    Encode.list encodeTask tasks


encodeTask : Task -> Encode.Value
encodeTask t =
    Encode.object
        [ ( "id", Encode.string t.id )
        , ( "title", Encode.string t.title )
        , ( "note", Encode.string t.note )
        , ( "done", Encode.bool t.done )
        , ( "createdAt", Encode.string t.createdAt )
        , ( "completedAt", Maybe.withDefault Encode.null (Maybe.map Encode.string t.completedAt) )
        , ( "day", Encode.string t.day )
        , ( "order", Encode.int t.order )
        ]



-- VIEW


view : Model -> Html Msg
view model =
    -- The clay accent (--accent) and tick colour (--tick) are defined on `.app`
    -- in app.css. They are intentionally NOT set inline here: Elm 0.19's
    -- `Html.Attributes.style` assigns `node.style[key]` directly, which does not
    -- register CSS custom properties (that needs `setProperty`), so an inline
    -- `style "--accent" …` silently does nothing.
    div
        [ A.class "app"
        , A.attribute "data-look" "paper"
        , A.attribute "data-density" "regular"
        ]
        [ div [ A.class "col" ]
            [ viewHeader model
            , case model.view of
                TodayView ->
                    viewList model

                CalendarView ->
                    viewCalendar model
            ]
        , viewMentionMenu model
        ]


viewHeader : Model -> Html Msg
viewHeader model =
    let
        todays =
            List.filter (\t -> t.day == model.today) model.tasks

        done =
            List.length (List.filter .done todays)

        total =
            List.length todays

        eyebrow =
            case model.view of
                TodayView ->
                    DateUtil.relativeLabel model.today model.today

                CalendarView ->
                    "History"
    in
    header [ A.class "hd" ]
        [ div [ A.class "hd-date" ]
            [ span [ A.class "hd-eyebrow" ] [ text eyebrow ]
            , h1 [ A.class "hd-weekday" ] [ text (DateUtil.weekdayName model.today) ]
            , span [ A.class "hd-full" ] [ text (DateUtil.prettyDate model.today) ]
            ]
        , div [ A.class "hd-right" ]
            [ if model.view == TodayView && total > 0 then
                viewProgress done total

              else
                text ""
            , viewSegmented model.view
            ]
        ]


viewProgress : Int -> Int -> Html Msg
viewProgress done total =
    let
        circ =
            2 * pi * 15

        frac =
            if total == 0 then
                0

            else
                toFloat done / toFloat total

        offset =
            circ * (1 - frac)
    in
    div [ A.class "progress" ]
        [ span [ A.class "progress-num" ]
            [ text (String.fromInt done)
            , span [ A.class "progress-slash" ] [ text "/" ]
            , text (String.fromInt total)
            ]
        , span [ A.class "progress-ring", A.attribute "aria-hidden" "true" ]
            [ Svg.svg [ SA.viewBox "0 0 36 36", SA.width "34", SA.height "34" ]
                [ Svg.circle [ SA.class "ring-bg", SA.cx "18", SA.cy "18", SA.r "15" ] []
                , Svg.circle
                    [ SA.class "ring-fg"
                    , SA.cx "18"
                    , SA.cy "18"
                    , SA.r "15"
                    , SA.strokeDasharray (String.fromFloat circ)
                    , SA.strokeDashoffset (String.fromFloat offset)
                    ]
                    []
                ]
            ]
        ]


viewSegmented : ViewMode -> Html Msg
viewSegmented current =
    div [ A.class "seg", A.attribute "role" "tablist" ]
        [ button
            [ A.classList [ ( "seg-btn", True ), ( "is-on", current == TodayView ) ]
            , Ev.onClick (SetView TodayView)
            ]
            [ strokeSvg "15" "1.7" [ Svg.path [ SA.d "M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01" ] [] ]
            , span [] [ text "List" ]
            ]
        , button
            [ A.classList [ ( "seg-btn", True ), ( "is-on", current == CalendarView ) ]
            , Ev.onClick (SetView CalendarView)
            ]
            [ strokeSvg "15" "1.7"
                [ Svg.rect [ SA.x "3", SA.y "4.5", SA.width "18", SA.height "16", SA.rx "2" ] []
                , Svg.path [ SA.d "M3 9h18M8 2.5v4M16 2.5v4" ] []
                ]
            , span [] [ text "Calendar" ]
            ]
        ]



-- LIST VIEW


viewList : Model -> Html Msg
viewList model =
    let
        todays =
            List.filter (\t -> t.day == model.today) model.tasks

        carried =
            todays
                |> List.filter (\t -> not t.done && t.createdAt < model.today)
                |> List.sortBy .order

        fresh =
            todays
                |> List.filter (\t -> not t.done && t.createdAt >= model.today)
                |> List.sortBy .order

        doneToday =
            todays
                |> List.filter .done
                |> List.sortBy .order

        openCount =
            List.length carried + List.length fresh

        isEmpty =
            List.isEmpty todays
    in
    div [ A.class "list-view" ]
        [ if List.isEmpty carried then
            text ""

          else
            section [ A.class "group group-carried" ]
                [ groupHead "Carried over" (Just (List.length carried))
                , div [ A.class "group-rows" ]
                    (List.map (viewTask model True) carried)
                , viewDropEnd model carried
                ]
        , section [ A.class "group" ]
            [ if List.isEmpty carried then
                text ""

              else
                groupHead "Today" Nothing
            , div [ A.class "group-rows" ]
                (List.map (viewTask model False) fresh)
            , viewDropEnd model fresh
            , if isEmpty then
                viewEmpty "A clear day." "Add the first thing below."

              else if openCount == 0 then
                viewEmpty "All done for today."
                    ("Nice work — " ++ String.fromInt (List.length doneToday) ++ " complete.")

              else
                text ""
            , viewAddRow model
            ]
        , if List.isEmpty doneToday then
            text ""

          else
            section [ A.class "group group-completed" ]
                [ groupHead "Completed" (Just (List.length doneToday))
                , div [ A.class "group-rows" ]
                    (List.map (viewTask model False) doneToday)
                ]
        ]


groupHead : String -> Maybe Int -> Html Msg
groupHead label count =
    div [ A.class "group-head" ]
        (span [ A.class "eyebrow" ] [ text label ]
            :: (case count of
                    Just n ->
                        [ span [ A.class "group-count" ] [ text (String.fromInt n) ] ]

                    Nothing ->
                        []
               )
        )


viewEmpty : String -> String -> Html Msg
viewEmpty line sub =
    div [ A.class "empty" ]
        [ p [ A.class "empty-line" ] [ text line ]
        , p [ A.class "empty-sub" ] [ text sub ]
        ]


viewDropEnd : Model -> List Task -> Html Msg
viewDropEnd model tasks =
    case ( model.draggingId, List.reverse tasks |> List.head ) of
        ( Just _, Just anchor ) ->
            div
                [ A.classList
                    [ ( "task-drop-end", True )
                    , ( "is-drop-target", model.dropAfterId == Just anchor.id )
                    ]
                , A.attribute "data-drop-after-id" anchor.id
                ]
                []

        _ ->
            text ""


viewAddRow : Model -> Html Msg
viewAddRow model =
    div [ A.class "add-row" ]
        [ span [ A.class "add-plus", A.attribute "aria-hidden" "true" ]
            [ strokeSvg "18" "1.7" [ Svg.path [ SA.d "M12 5v14M5 12h14" ] [] ] ]
        , viewMentionField model
            { field = AddField
            , inputClass = "add-input"
            , value = model.addInput
            , placeholder = "Add a task…"
            , rows = 1
            , autosize = True
            , enter = SubmitOnEnter
            , stopClicks = False
            }
        ]


{-| What Enter means in a field when the mention popup is closed. -}
type EnterBehavior
    = SubmitOnEnter
    | SwallowEnter
    | NewlineOnEnter


type alias FieldConfig =
    { field : MentionField
    , inputClass : String
    , value : String
    , placeholder : String
    , rows : Int
    , autosize : Bool
    , enter : EnterBehavior
    , stopClicks : Bool
    }


{-| A textarea with an inline chip layer.

The chips are drawn by a mirror div sitting exactly under a transparent-text
textarea — the browser has no way to put real elements inside a text control,
and a contenteditable would mean owning selection and undo by hand. The mirror
shares the field's class so its metrics match glyph for glyph, and the chip's
padding is faked with a box-shadow ring so highlighting a name never shifts the
text under the caret.

mentions.js measures the caret against the same mirror, which is why every
segment reproduces its source text verbatim.

-}
viewMentionField : Model -> FieldConfig -> Html Msg
viewMentionField model cfg =
    let
        id =
            fieldId cfg.field
    in
    div [ A.class "mfield" ]
        [ div
            [ A.id (id ++ "-mirror")
            , A.class ("mfield-mirror " ++ cfg.inputClass)
            , A.attribute "aria-hidden" "true"
            ]
            (viewChips cfg.value ++ [ text "\u{200B}" ])
        , textarea
            (List.append
                [ A.id id
                , A.class (cfg.inputClass ++ " mfield-input")
                , A.value cfg.value
                , A.placeholder cfg.placeholder
                , A.rows cfg.rows
                , A.spellcheck False
                , A.attribute "wrap" "soft"
                , onFieldInput cfg.field
                , onFieldClick cfg.stopClicks cfg.field
                , onFieldCaretKeys cfg.field
                , onFieldKeyDown model cfg.field cfg.enter
                , Ev.on "blur" (Decode.succeed MentionClose)
                ]
                (if cfg.autosize then
                    [ A.attribute "data-autosize" "title" ]

                 else
                    []
                )
            )
            []
        ]


viewChips : String -> List (Html msg)
viewChips value =
    Mention.segments value
        |> List.map
            (\segment ->
                case segment of
                    Mention.Plain plain ->
                        text plain

                    Mention.Chip handle ->
                        span [ A.class "mention-chip" ] [ text ("@" ++ handle) ]
            )


{-| Popup geometry, in step with the .mention-menu rule in app.css. -}
menuWidth : Float
menuWidth =
    260


menuRowHeight : Float
menuRowHeight =
    32


menuPadding : Float
menuPadding =
    8


menuGap : Float
menuGap =
    7


viewMentionMenu : Model -> Html Msg
viewMentionMenu model =
    case Maybe.andThen (\menu -> Maybe.map (Tuple.pair menu) menu.pos) model.mention of
        Just ( menu, pos ) ->
            case menuItems model menu of
                [] ->
                    text ""

                items ->
                    let
                        height =
                            menuPadding + toFloat (List.length items) * menuRowHeight

                        -- Drop below the line, unless that would run off the
                        -- bottom, in which case sit above it.
                        top =
                            if pos.y + menuGap + height > pos.viewHeight - 8 then
                                max 8 (pos.lineTop - menuGap - height)

                            else
                                pos.y + menuGap

                        left =
                            clamp 8 (max 8 (pos.viewWidth - menuWidth - 8)) pos.x
                    in
                    div
                        [ A.class "mention-menu"
                        , A.style "left" (String.fromFloat left ++ "px")
                        , A.style "top" (String.fromFloat top ++ "px")
                        , A.attribute "role" "listbox"
                        ]
                        (List.indexedMap (viewMentionItem menu.index) items)

        Nothing ->
            text ""


viewMentionItem : Int -> Int -> String -> Html Msg
viewMentionItem selected index handle =
    button
        [ A.classList [ ( "mention-item", True ), ( "is-active", index == selected ) ]
        , A.attribute "role" "option"

        -- Swallow the mousedown so the field keeps focus and the blur that
        -- would close this menu never fires before the click lands.
        , Ev.preventDefaultOn "mousedown" (Decode.succeed ( MentionHover index, True ))
        , Ev.onClick (MentionPick handle)
        , Ev.onMouseEnter (MentionHover index)
        ]
        [ span [ A.class "mention-item-at" ] [ text "@" ], text handle ]



-- FIELD EVENTS


targetValue : Decode.Decoder String
targetValue =
    Decode.at [ "target", "value" ] Decode.string


targetCaret : Decode.Decoder Int
targetCaret =
    Decode.at [ "target", "selectionStart" ] Decode.int


onFieldInput : MentionField -> Html.Attribute Msg
onFieldInput field =
    Ev.on "input" (Decode.map2 (FieldInput field) targetValue targetCaret)


onFieldClick : Bool -> MentionField -> Html.Attribute Msg
onFieldClick stop field =
    Ev.stopPropagationOn "click"
        (Decode.map2 (FieldCaret field) targetValue targetCaret
            |> Decode.map (\msg -> ( msg, stop ))
        )


{-| Arrowing or jumping the caret can move it into or out of a mention without
changing a character, so those keys re-check on the way up. Everything else is
already covered by the input event — and reading the DOM after Enter would see
the value Elm is about to replace.
-}
onFieldCaretKeys : MentionField -> Html.Attribute Msg
onFieldCaretKeys field =
    Ev.on "keyup"
        (Decode.field "key" Decode.string
            |> Decode.andThen
                (\key ->
                    if List.member key [ "ArrowLeft", "ArrowRight", "Home", "End" ] then
                        Decode.map2 (FieldCaret field) targetValue targetCaret

                    else
                        Decode.fail "not a caret key"
                )
        )


{-| While the popup is up it owns the arrows, Tab and Escape. Those are decided
from the last render, which is fine — being a frame stale only costs a caret
move or a focus change. Enter is not: it goes to `FieldEnter`, which re-reads
the field and works out what the key meant there.
-}
onFieldKeyDown : Model -> MentionField -> EnterBehavior -> Html.Attribute Msg
onFieldKeyDown model field enter =
    let
        menuOpen =
            case model.mention of
                Just menu ->
                    (menu.field == field)
                        && (menu.pos /= Nothing)
                        && not (List.isEmpty (menuItems model menu))

                Nothing ->
                    False

        onEnter =
            Decode.map2 (FieldEnter field) targetValue targetCaret
                |> Decode.map (\msg -> ( msg, True ))
    in
    Ev.preventDefaultOn "keydown"
        (Decode.field "key" Decode.string
            |> Decode.andThen
                (\key ->
                    if key == "Enter" then
                        if menuOpen || enter /= NewlineOnEnter then
                            onEnter

                        else
                            Decode.fail "let the newline through"

                    else if menuOpen then
                        case key of
                            "ArrowDown" ->
                                Decode.succeed ( MentionMove 1, True )

                            "ArrowUp" ->
                                Decode.succeed ( MentionMove -1, True )

                            "Tab" ->
                                Decode.succeed ( MentionAccept, True )

                            "Escape" ->
                                Decode.succeed ( MentionClose, True )

                            _ ->
                                Decode.fail "not a menu key"

                    else
                        Decode.fail "not handled"
                )
        )


viewTask : Model -> Bool -> Task -> Html Msg
viewTask model carried task =
    let
        isOpen =
            model.openId == Just task.id

        hasNote =
            String.trim task.note /= ""

        age =
            if carried then
                DateUtil.daysBetween task.createdAt model.today

            else
                0

        firstLine =
            String.lines task.note |> List.head |> Maybe.withDefault ""
    in
    div
        [ A.classList
            [ ( "task", True )
            , ( "is-done", task.done )
            , ( "is-open", isOpen )
            , ( "is-dragging", model.draggingId == Just task.id )
            , ( "is-drop-target", model.dragOverId == Just task.id && model.draggingId /= Just task.id )
            ]
        , A.attribute "data-task-id" task.id
        ]
        [ div [ A.class "task-main" ]
            [ if task.done then
                text ""

              else
                viewDragHandle task
            , viewCheck task
            , div [ A.class "task-body", Ev.onClick (ToggleOpen task.id) ]
                [ viewMentionField model
                    { field = TitleField task.id
                    , inputClass = "task-title"
                    , value = task.title
                    , placeholder = ""
                    , rows = taskTitleRows task.title
                    , autosize = True
                    , enter = SwallowEnter
                    , stopClicks = True
                    }
                , if hasNote && not isOpen then
                    span [ A.class "task-note-preview" ] (viewChips firstLine)

                  else
                    text ""
                ]
            , div [ A.class "task-meta" ]
                [ if carried && age > 0 then
                    span
                        [ A.class "age-tag"
                        , A.title ("Carried from " ++ DateUtil.prettyDate task.createdAt)
                        ]
                        [ text (String.fromInt age ++ "d") ]

                  else
                    text ""
                , button
                    [ A.class "note-btn"
                    , A.attribute "aria-label" "Notes"
                    , A.attribute "data-has-note"
                        (if hasNote then
                            "1"

                         else
                            "0"
                        )
                    , Ev.onClick (ToggleOpen task.id)
                    ]
                    [ strokeSvg "15" "1.6" [ Svg.path [ SA.d "M4 6h16M4 12h16M4 18h10" ] [] ] ]
                , button
                    [ A.class "del-btn"
                    , A.attribute "aria-label" "Delete task"
                    , Ev.onClick (Delete task.id)
                    ]
                    [ strokeSvg "15" "1.6" [ Svg.path [ SA.d "M18 6 6 18M6 6l12 12" ] [] ] ]
                ]
            ]
        , if isOpen then
            div [ A.class "task-note-wrap" ]
                [ viewMentionField model
                    { field = NoteField task.id
                    , inputClass = "task-note"
                    , value = task.note
                    , placeholder = "Add a note or list sub-steps, one per line…"
                    , rows = max 1 (List.length (String.lines task.note))
                    , autosize = False
                    , enter = NewlineOnEnter
                    , stopClicks = True
                    }
                ]

          else
            text ""
        ]


viewDragHandle : Task -> Html Msg
viewDragHandle task =
    span
        [ A.class "drag-handle"
        , A.attribute "role" "button"
        , A.attribute "aria-label" "Drag to reorder task"
        , A.title "Drag to reorder"
        ]
        [ strokeSvg "18" "1.7"
            [ Svg.path [ SA.d "M7 5h.01M12 5h.01M17 5h.01M7 9h.01M12 9h.01M17 9h.01M7 13h.01M12 13h.01M17 13h.01M7 17h.01M12 17h.01M17 17h.01" ] [] ]
        ]


taskTitleRows : String -> Int
taskTitleRows title =
    title
        |> String.lines
        |> List.map (\line -> max 1 ((String.length line + 24) // 25))
        |> List.sum
        |> max 1


viewCheck : Task -> Html Msg
viewCheck task =
    button
        [ A.classList [ ( "check", True ), ( "is-done", task.done ) ]
        , A.attribute "aria-label"
            (if task.done then
                "Mark as not done"

             else
                "Mark as done"
            )
        , Ev.stopPropagationOn "click" (Decode.succeed ( Toggle task.id, True ))
        ]
        [ Svg.svg [ SA.viewBox "0 0 24 24", SA.class "check-tick", A.attribute "aria-hidden" "true" ]
            [ Svg.path
                [ SA.d "M5 12.5 L10 17.5 L19 7"
                , SA.fill "none"
                , SA.stroke "currentColor"
                , SA.strokeWidth "2.5"
                , SA.strokeLinecap "round"
                , SA.strokeLinejoin "round"
                ]
                []
            ]
        ]



-- CALENDAR VIEW


viewCalendar : Model -> Html Msg
viewCalendar model =
    let
        ( y, m ) =
            model.calCursor

        byDay =
            dayCounts model.tasks

        firstIso =
            DateUtil.toISO y (m + 1) 1

        startPad =
            DateUtil.weekdayIndex firstIso

        days =
            DateUtil.daysInMonth y (m + 1)

        cells =
            List.repeat startPad Nothing
                ++ List.map Just (List.range 1 days)
    in
    div [ A.class "cal-view" ]
        [ div [ A.class "cal-head" ]
            [ span [ A.class "cal-month" ] [ text (DateUtil.monthName m ++ " " ++ String.fromInt y) ]
            , div [ A.class "cal-nav" ]
                [ button [ A.attribute "aria-label" "Previous month", Ev.onClick (CalShift -1) ]
                    [ strokeSvg "16" "1.7" [ Svg.path [ SA.d "M15 18l-6-6 6-6" ] [] ] ]
                , button [ A.attribute "aria-label" "Next month", Ev.onClick (CalShift 1) ]
                    [ strokeSvg "16" "1.7" [ Svg.path [ SA.d "M9 18l6-6-6-6" ] [] ] ]
                ]
            ]
        , div [ A.class "cal-grid" ]
            (List.map (\i -> div [ A.class "cal-dow" ] [ text (String.left 1 (DateUtil.weekdayShort i)) ]) (List.range 0 6)
                ++ List.indexedMap (viewCalCell model byDay) cells
            )
        , viewCalDetail model
        ]


viewCalCell : Model -> Dict String DayCount -> Int -> Maybe Int -> Html Msg
viewCalCell model byDay idx maybeDay =
    case maybeDay of
        Nothing ->
            div [ A.class "cal-cell is-pad", A.attribute "data-pad" (String.fromInt idx) ] []

        Just d ->
            let
                ( y, m ) =
                    model.calCursor

                iso =
                    DateUtil.toISO y (m + 1) d

                info =
                    Dict.get iso byDay |> Maybe.withDefault { done = 0, open = 0 }

                isToday =
                    iso == model.today

                isSel =
                    iso == model.calSelected

                future =
                    iso > model.today
            in
            button
                [ A.classList
                    [ ( "cal-cell", True )
                    , ( "is-today", isToday )
                    , ( "is-sel", isSel )
                    , ( "is-future", future )
                    ]
                , Ev.onClick (CalSelect iso)
                ]
                [ span [ A.class "cal-num" ] [ text (String.fromInt d) ]
                , if info.done > 0 then
                    span [ A.class "cal-dots" ]
                        (List.repeat (min info.done 4) (span [ A.class "cal-dot" ] []))

                  else if info.open > 0 && not future then
                    span [ A.class "cal-dots" ] [ span [ A.class "cal-dot is-open" ] [] ]

                  else
                    text ""
                ]


viewCalDetail : Model -> Html Msg
viewCalDetail model =
    let
        sel =
            model.calSelected

        selDone =
            model.tasks
                |> List.filter (\t -> t.done && t.completedAt == Just sel)
                |> List.sortBy .order

        selOpen =
            model.tasks
                |> List.filter (\t -> not t.done && t.day == sel)
                |> List.sortBy .order

        isFuture =
            sel > model.today

        body =
            if isFuture then
                [ p [ A.class "detail-empty" ] [ text "Nothing scheduled yet." ] ]

            else if List.isEmpty selDone && List.isEmpty selOpen then
                [ p [ A.class "detail-empty" ] [ text "A quiet day — nothing logged." ] ]

            else
                [ div [ A.class "detail-list" ]
                    (List.map (viewDetailItem True) selDone
                        ++ List.map (viewDetailItem False) selOpen
                    )
                ]
    in
    div [ A.class "cal-detail" ]
        (div [ A.class "detail-head" ]
            [ span [ A.class "detail-day" ] [ text (DateUtil.weekdayName sel) ]
            , span [ A.class "detail-date" ] [ text (DateUtil.prettyDate sel) ]
            ]
            :: body
        )


viewDetailItem : Bool -> Task -> Html Msg
viewDetailItem isDone task =
    div [ A.classList [ ( "detail-item", True ), ( "is-done", isDone ) ] ]
        [ if isDone then
            span [ A.class "detail-tick" ]
                [ Svg.svg
                    [ SA.viewBox "0 0 24 24", SA.width "13", SA.height "13", SA.fill "none", SA.stroke "currentColor", SA.strokeWidth "2.6", SA.strokeLinecap "round", SA.strokeLinejoin "round" ]
                    [ Svg.path [ SA.d "M5 12.5 L10 17.5 L19 7" ] [] ]
                ]

          else
            span [ A.class "detail-tick is-open" ] []
        , span [ A.class "detail-title" ] (viewChips task.title)
        ]



-- CALENDAR DATA


type alias DayCount =
    { done : Int, open : Int }


dayCounts : List Task -> Dict String DayCount
dayCounts tasks =
    let
        bump : (DayCount -> DayCount) -> String -> Dict String DayCount -> Dict String DayCount
        bump f key dict =
            Dict.update key
                (\maybe ->
                    Just (f (Maybe.withDefault { done = 0, open = 0 } maybe))
                )
                dict
    in
    List.foldl
        (\t acc ->
            let
                afterDone =
                    case ( t.done, t.completedAt ) of
                        ( True, Just c ) ->
                            bump (\dc -> { dc | done = dc.done + 1 }) c acc

                        _ ->
                            acc
            in
            if not t.done then
                bump (\dc -> { dc | open = dc.open + 1 }) t.day afterDone

            else
                afterDone
        )
        Dict.empty
        tasks



-- SVG / EVENT HELPERS


{-| A 24×24 stroked-icon svg with rounded joins — the prototype's house style. -}
strokeSvg : String -> String -> List (Svg.Svg msg) -> Html msg
strokeSvg size sw children =
    Svg.svg
        [ SA.viewBox "0 0 24 24"
        , SA.width size
        , SA.height size
        , SA.fill "none"
        , SA.stroke "currentColor"
        , SA.strokeWidth sw
        , SA.strokeLinecap "round"
        , SA.strokeLinejoin "round"
        ]
        children


-- MAIN


main : Program Flags Model Msg
main =
    Browser.element
        { init = init
        , update = update
        , view = view
        , subscriptions =
            \_ ->
                Sub.batch
                    [ dbLoaded GotStored
                    , todayChanged GotToday
                    , taskDragStarted DragStart
                    , taskDragOver DragOver
                    , taskDragOverAfter DragOverAfter
                    , taskDropped Drop
                    , taskDroppedAfter DropAfter
                    , taskDragEnded (\_ -> DragEnd)
                    , caretPos GotCaretPos
                    ]
        }
