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
import Html exposing (Html, button, div, h1, header, input, p, section, span, text, textarea)
import Html.Attributes as A
import Html.Events as Ev
import Json.Decode as Decode
import Json.Encode as Encode
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
      }
    , dbLoad ()
    )



-- UPDATE


type Msg
    = NoOp
    | GotStored Encode.Value
    | GotToday String
    | SetView ViewMode
    | AddInput String
    | SubmitAdd
    | Toggle String
    | EditTitle String String
    | EditNote String String
    | ToggleOpen String
    | DragStart String
    | DragOver String
    | Drop String
    | DragEnd
    | Delete String
    | CalShift Int
    | CalSelect String


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        NoOp ->
            ( model, Cmd.none )

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

        AddInput s ->
            ( { model | addInput = s }, Cmd.none )

        SubmitAdd ->
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
                    }

        Toggle id ->
            persist (mapTask id (toggleTask model.today) model)

        EditTitle id value ->
            persist (mapTask id (\t -> { t | title = value }) model)

        EditNote id value ->
            persist (mapTask id (\t -> { t | note = value }) model)

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
            ( { model | draggingId = Just id, dragOverId = Nothing }, Cmd.none )

        DragOver id ->
            ( { model | dragOverId = Just id }, Cmd.none )

        Drop targetId ->
            case model.draggingId of
                Just draggedId ->
                    let
                        reordered =
                            reorderTaskBefore model.today draggedId targetId model
                    in
                    ( { reordered | draggingId = Nothing, dragOverId = Nothing }
                    , if reordered.tasks == model.tasks then
                        Cmd.none

                      else
                        dbSave (encodeTasks reordered.tasks)
                    )

                Nothing ->
                    ( model, Cmd.none )

        DragEnd ->
            ( { model | draggingId = Nothing, dragOverId = Nothing }, Cmd.none )

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


persist : Model -> ( Model, Cmd Msg )
persist model =
    ( model, dbSave (encodeTasks model.tasks) )


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
                ]
        , section [ A.class "group" ]
            [ if List.isEmpty carried then
                text ""

              else
                groupHead "Today" Nothing
            , div [ A.class "group-rows" ]
                (List.map (viewTask model False) fresh)
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


viewAddRow : Model -> Html Msg
viewAddRow model =
    div [ A.class "add-row" ]
        [ span [ A.class "add-plus", A.attribute "aria-hidden" "true" ]
            [ strokeSvg "18" "1.7" [ Svg.path [ SA.d "M12 5v14M5 12h14" ] [] ] ]
        , input
            [ A.class "add-input"
            , A.value model.addInput
            , A.placeholder "Add a task…"
            , A.spellcheck False
            , Ev.onInput AddInput
            , onEnter SubmitAdd
            ]
            []
        ]


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
        , Ev.preventDefaultOn "dragover" (Decode.succeed ( DragOver task.id, True ))
        , Ev.preventDefaultOn "drop" (Decode.succeed ( Drop task.id, True ))
        ]
        [ div [ A.class "task-main" ]
            [ if task.done then
                text ""

              else
                viewDragHandle task
            , viewCheck task
            , div [ A.class "task-body", Ev.onClick (ToggleOpen task.id) ]
                [ textarea
                    [ A.class "task-title"
                    , A.value task.title
                    , A.rows (taskTitleRows task.title)
                    , A.attribute "data-autosize" "title"
                    , A.attribute "wrap" "soft"
                    , A.spellcheck False
                    , Ev.onInput (EditTitle task.id)
                    , stopEnter
                    , stopClick
                    ]
                    []
                , if hasNote && not isOpen then
                    span [ A.class "task-note-preview" ] [ text firstLine ]

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
                [ textarea
                    [ A.class "task-note"
                    , A.value task.note
                    , A.placeholder "Add a note or list sub-steps, one per line…"
                    , A.spellcheck False
                    , A.rows (max 1 (List.length (String.lines task.note)))
                    , Ev.onInput (EditNote task.id)
                    ]
                    []
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
        , A.draggable "true"
        , Ev.on "dragstart" (Decode.succeed (DragStart task.id))
        , Ev.on "dragend" (Decode.succeed DragEnd)
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
        , span [ A.class "detail-title" ] [ text task.title ]
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


onEnter : Msg -> Html.Attribute Msg
onEnter msg =
    Ev.on "keydown"
        (Decode.field "key" Decode.string
            |> Decode.andThen
                (\key ->
                    if key == "Enter" then
                        Decode.succeed msg

                    else
                        Decode.fail "not Enter"
                )
        )


{-| Preserve one-line input semantics for wrapped title textareas. -}
stopEnter : Html.Attribute Msg
stopEnter =
    Ev.preventDefaultOn "keydown"
        (Decode.field "key" Decode.string
            |> Decode.andThen
                (\key ->
                    if key == "Enter" then
                        Decode.succeed ( NoOp, True )

                    else
                        Decode.fail "not Enter"
                )
        )


{-| Swallow clicks on the title control so editing doesn't toggle the note drawer. -}
stopClick : Html.Attribute Msg
stopClick =
    Ev.stopPropagationOn "click" (Decode.succeed ( NoOp, True ))



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
                    ]
        }
