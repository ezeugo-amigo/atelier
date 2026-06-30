port module Main exposing (main)

import Browser
import Dict exposing (Dict)
import Html exposing (Html, button, div, h1, h2, input, label, p, span, strong, text, textarea)
import Html.Attributes as A
import Html.Events as Ev
import Json.Decode as Decode exposing (Decoder)
import Json.Encode as Encode
import Svg
import Svg.Attributes as SA



-- PORTS


port commandOut : Encode.Value -> Cmd msg


port commandIn : (Encode.Value -> msg) -> Sub msg



-- MODEL


type alias Account =
    { id : String
    , displayName : String
    , emailAddress : String
    , provider : String
    , accent : String
    }


type alias Folder =
    { id : String
    , accountId : String
    , name : String
    , role : String
    , unreadCount : Int
    }


type alias MessageSummary =
    { id : String
    , accountId : String
    , folderId : String
    , senderName : String
    , senderEmail : String
    , subject : String
    , snippet : String
    , receivedAt : String
    , unread : Bool
    , starred : Bool
    , labels : List String
    }


type alias MessageDetail =
    { id : String
    , accountId : String
    , folderId : String
    , senderName : String
    , senderEmail : String
    , subject : String
    , snippet : String
    , receivedAt : String
    , unread : Bool
    , starred : Bool
    , labels : List String
    , to : List String
    , cc : List String
    , bodyParagraphs : List String
    }


type alias SyncStatus =
    { state : String
    , lastChecked : String
    , detail : String
    }


type alias BootstrapData =
    { accounts : List Account
    , folders : List Folder
    , messages : List MessageSummary
    , selectedFolderId : String
    , selectedMessageId : Maybe String
    , selectedMessage : Maybe MessageDetail
    , syncStatus : SyncStatus
    }


type alias MailboxSnapshot =
    { folderId : String
    , folders : List Folder
    , messages : List MessageSummary
    , selectedMessageId : Maybe String
    , selectedMessage : Maybe MessageDetail
    , syncStatus : SyncStatus
    }


type alias MessageUpdate =
    { folders : List Folder
    , message : MessageDetail
    , syncStatus : SyncStatus
    }


type RequestKind
    = BootstrapRequest
    | SelectFolderRequest
    | SelectMessageRequest
    | SearchRequest
    | MarkReadRequest
    | ArchiveRequest
    | RefreshRequest


type alias Model =
    { accounts : List Account
    , folders : List Folder
    , messages : List MessageSummary
    , selectedFolderId : Maybe String
    , selectedMessageId : Maybe String
    , selectedMessage : Maybe MessageDetail
    , syncStatus : SyncStatus
    , search : String
    , booted : Bool
    , loading : Bool
    , error : Maybe String
    , pending : Dict Int RequestKind
    , nextRequestId : Int
    , composeOpen : Bool
    , composeTo : String
    , composeSubject : String
    , composeBody : String
    }


type alias Flags =
    {}


emptySync : SyncStatus
emptySync =
    { state = "Starting"
    , lastChecked = "-"
    , detail = "Opening local mailbox"
    }


initialModel : Model
initialModel =
    { accounts = []
    , folders = []
    , messages = []
    , selectedFolderId = Nothing
    , selectedMessageId = Nothing
    , selectedMessage = Nothing
    , syncStatus = emptySync
    , search = ""
    , booted = False
    , loading = True
    , error = Nothing
    , pending = Dict.singleton 1 BootstrapRequest
    , nextRequestId = 2
    , composeOpen = False
    , composeTo = ""
    , composeSubject = ""
    , composeBody = ""
    }



-- INIT


main : Program Flags Model Msg
main =
    Browser.element
        { init = init
        , update = update
        , subscriptions = subscriptions
        , view = view
        }


init : Flags -> ( Model, Cmd Msg )
init _ =
    ( initialModel, sendCommand 1 "app_bootstrap" Encode.null )



-- UPDATE


type Msg
    = GotCommand Encode.Value
    | SelectFolder String
    | SelectMessage String
    | SearchInput String
    | RunSearch
    | Refresh
    | ToggleSelectedRead
    | ArchiveSelected
    | OpenCompose
    | CloseCompose
    | ComposeTo String
    | ComposeSubject String
    | ComposeBody String
    | SendCompose
    | NoOp


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        GotCommand value ->
            handleCommand value model

        SelectFolder folderId ->
            enqueue SelectFolderRequest "select_folder" (Encode.object [ ( "folderId", Encode.string folderId ) ])
                { model | search = "" }

        SelectMessage messageId ->
            enqueue SelectMessageRequest "select_message" (Encode.object [ ( "messageId", Encode.string messageId ) ]) model

        SearchInput value ->
            ( { model | search = value }, Cmd.none )

        RunSearch ->
            enqueue SearchRequest "search_messages" (Encode.object [ ( "query", Encode.string model.search ) ]) model

        Refresh ->
            enqueue RefreshRequest "refresh_mail" Encode.null model

        ToggleSelectedRead ->
            case model.selectedMessage of
                Nothing ->
                    ( model, Cmd.none )

                Just message ->
                    enqueue MarkReadRequest
                        "mark_message_read"
                        (Encode.object
                            [ ( "messageId", Encode.string message.id )
                            , ( "read", Encode.bool message.unread )
                            ]
                        )
                        model

        ArchiveSelected ->
            case model.selectedMessageId of
                Nothing ->
                    ( model, Cmd.none )

                Just messageId ->
                    enqueue ArchiveRequest "archive_message" (Encode.object [ ( "messageId", Encode.string messageId ) ]) model

        OpenCompose ->
            ( { model | composeOpen = True }, Cmd.none )

        CloseCompose ->
            ( { model | composeOpen = False }, Cmd.none )

        ComposeTo value ->
            ( { model | composeTo = value }, Cmd.none )

        ComposeSubject value ->
            ( { model | composeSubject = value }, Cmd.none )

        ComposeBody value ->
            ( { model | composeBody = value }, Cmd.none )

        SendCompose ->
            ( { model
                | composeOpen = False
                , composeTo = ""
                , composeSubject = ""
                , composeBody = ""
                , syncStatus =
                    { state = "Drafted"
                    , lastChecked = "Now"
                    , detail = "Message staged locally"
                    }
              }
            , Cmd.none
            )

        NoOp ->
            ( model, Cmd.none )


enqueue : RequestKind -> String -> Encode.Value -> Model -> ( Model, Cmd Msg )
enqueue kind command payload model =
    let
        requestId =
            model.nextRequestId
    in
    ( { model
        | nextRequestId = requestId + 1
        , pending = Dict.insert requestId kind model.pending
        , loading = True
        , error = Nothing
      }
    , sendCommand requestId command payload
    )


sendCommand : Int -> String -> Encode.Value -> Cmd Msg
sendCommand requestId command payload =
    commandOut
        (Encode.object
            [ ( "requestId", Encode.int requestId )
            , ( "command", Encode.string command )
            , ( "payload", payload )
            ]
        )


subscriptions : Model -> Sub Msg
subscriptions _ =
    commandIn GotCommand



-- COMMAND RESPONSES


type alias BridgeResponse =
    { requestId : Int
    , command : String
    , ok : Bool
    , data : Maybe Encode.Value
    , error : Maybe String
    }


handleCommand : Encode.Value -> Model -> ( Model, Cmd Msg )
handleCommand value model =
    case Decode.decodeValue bridgeResponseDecoder value of
        Err error ->
            ( { model | loading = False, error = Just (Decode.errorToString error) }, Cmd.none )

        Ok response ->
            let
                maybeKind =
                    Dict.get response.requestId model.pending

                nextModel =
                    { model | pending = Dict.remove response.requestId model.pending }
            in
            case maybeKind of
                Nothing ->
                    ( nextModel, Cmd.none )

                Just kind ->
                    if response.ok then
                        applyResponse kind response nextModel

                    else
                        ( { nextModel
                            | loading = False
                            , error = Just (Maybe.withDefault ("Command failed: " ++ response.command) response.error)
                          }
                        , Cmd.none
                        )


applyResponse : RequestKind -> BridgeResponse -> Model -> ( Model, Cmd Msg )
applyResponse kind response model =
    case response.data of
        Nothing ->
            ( { model | loading = False, error = Just ("Missing response data for " ++ response.command) }, Cmd.none )

        Just data ->
            case kind of
                BootstrapRequest ->
                    decodeInto bootstrapDecoder applyBootstrap data model

                RefreshRequest ->
                    decodeInto bootstrapDecoder applyBootstrap data model

                SelectFolderRequest ->
                    decodeInto snapshotDecoder applySnapshot data model

                SearchRequest ->
                    decodeInto snapshotDecoder applySnapshot data model

                ArchiveRequest ->
                    decodeInto snapshotDecoder applySnapshot data model

                SelectMessageRequest ->
                    decodeInto messageUpdateDecoder applyMessageUpdate data model

                MarkReadRequest ->
                    decodeInto messageUpdateDecoder applyMessageUpdate data model


decodeInto : Decoder a -> (a -> Model -> Model) -> Encode.Value -> Model -> ( Model, Cmd Msg )
decodeInto decoder apply data model =
    case Decode.decodeValue decoder data of
        Ok decoded ->
            ( apply decoded { model | loading = False, error = Nothing }, Cmd.none )

        Err error ->
            ( { model | loading = False, error = Just (Decode.errorToString error) }, Cmd.none )


applyBootstrap : BootstrapData -> Model -> Model
applyBootstrap data model =
    { model
        | accounts = data.accounts
        , folders = data.folders
        , messages = data.messages
        , selectedFolderId = Just data.selectedFolderId
        , selectedMessageId = data.selectedMessageId
        , selectedMessage = data.selectedMessage
        , syncStatus = data.syncStatus
        , booted = True
    }


applySnapshot : MailboxSnapshot -> Model -> Model
applySnapshot snapshot model =
    { model
        | folders = snapshot.folders
        , messages = snapshot.messages
        , selectedFolderId = Just snapshot.folderId
        , selectedMessageId = snapshot.selectedMessageId
        , selectedMessage = snapshot.selectedMessage
        , syncStatus = snapshot.syncStatus
    }


applyMessageUpdate : MessageUpdate -> Model -> Model
applyMessageUpdate updateData model =
    let
        summary =
            detailToSummary updateData.message

        replace message =
            if message.id == summary.id then
                summary

            else
                message
    in
    { model
        | folders = updateData.folders
        , messages = List.map replace model.messages
        , selectedMessageId = Just updateData.message.id
        , selectedMessage = Just updateData.message
        , syncStatus = updateData.syncStatus
    }


detailToSummary : MessageDetail -> MessageSummary
detailToSummary message =
    { id = message.id
    , accountId = message.accountId
    , folderId = message.folderId
    , senderName = message.senderName
    , senderEmail = message.senderEmail
    , subject = message.subject
    , snippet = message.snippet
    , receivedAt = message.receivedAt
    , unread = message.unread
    , starred = message.starred
    , labels = message.labels
    }



-- VIEW


view : Model -> Html Msg
view model =
    div [ A.class "app" ]
        ([ viewSidebar model
         , viewListPane model
         , viewDetailPane model
         ]
            ++ (if model.composeOpen then
                    [ viewComposer model ]

                else
                    []
               )
        )


viewSidebar : Model -> Html Msg
viewSidebar model =
    div [ A.class "sidebar" ]
        [ div [ A.class "brand" ]
            [ div [ A.class "brand-title" ]
                [ span [ A.class "brand-mark" ] [ icon "mail" ]
                , span [] [ text "Maildesk" ]
                ]
            , div [ A.class "sidebar-actions" ]
                [ button [ A.class "icon-button", A.title "Refresh", Ev.onClick Refresh ] [ icon "refresh" ]
                , button [ A.class "icon-button", A.title "Compose", Ev.onClick OpenCompose ] [ icon "edit" ]
                ]
            ]
        , div [ A.class "nav-section" ]
            [ div [ A.class "nav-heading" ] [ text "Accounts" ]
            , div [] (List.map viewAccount model.accounts)
            ]
        , div [ A.class "nav-section" ]
            [ div [ A.class "nav-heading" ] [ text "Mailboxes" ]
            , div [] (List.map (viewFolder model.selectedFolderId) model.folders)
            ]
        , div [ A.class "sync-strip" ]
            [ div [ A.class "sync-line" ]
                [ span [ A.class "sync-state" ] [ text model.syncStatus.state ]
                , span [] [ text model.syncStatus.lastChecked ]
                ]
            , div [] [ text model.syncStatus.detail ]
            ]
        ]


viewAccount : Account -> Html Msg
viewAccount account =
    button [ A.class "account-row", Ev.onClick NoOp ]
        [ span [ A.style "color" account.accent ] [ icon "dot" ]
        , span [ A.class "account-name" ] [ text account.displayName ]
        , span [ A.class "date" ] [ text account.provider ]
        ]


viewFolder : Maybe String -> Folder -> Html Msg
viewFolder selected folder =
    button
        [ A.classList
            [ ( "folder-row", True )
            , ( "active", selected == Just folder.id )
            ]
        , Ev.onClick (SelectFolder folder.id)
        ]
        [ span [] [ iconForRole folder.role ]
        , span [ A.class "folder-name" ] [ text folder.name ]
        , if folder.unreadCount > 0 then
            span [ A.class "count" ] [ text (String.fromInt folder.unreadCount) ]

          else
            span [] []
        ]


viewListPane : Model -> Html Msg
viewListPane model =
    div [ A.class "list-pane" ]
        [ div [ A.class "toolbar" ]
            [ div [ A.class "search" ]
                [ icon "search"
                , input
                    [ A.placeholder "Search mail"
                    , A.value model.search
                    , Ev.onInput SearchInput
                    , onEnter RunSearch
                    ]
                    []
                ]
            , button [ A.class "icon-button", A.title "Run search", Ev.onClick RunSearch ] [ icon "arrow-right" ]
            ]
        , div [ A.class "folder-title" ]
            [ h1 [] [ text (currentFolderName model) ]
            , div [ A.class "folder-meta" ] [ text (folderMeta model) ]
            ]
        , div [ A.class "message-list" ]
            (if List.isEmpty model.messages then
                [ div [ A.class "empty-state" ]
                    [ div []
                        [ h2 [] [ text "No messages" ]
                        , p [] [ text "This mailbox is clear." ]
                        ]
                    ]
                ]

             else
                List.map (viewMessageRow model.selectedMessageId) model.messages
            )
        ]


viewMessageRow : Maybe String -> MessageSummary -> Html Msg
viewMessageRow selected message =
    button
        [ A.classList
            [ ( "message-row", True )
            , ( "unread", message.unread )
            , ( "selected", selected == Just message.id )
            ]
        , Ev.onClick (SelectMessage message.id)
        ]
        [ div [ A.class "sender" ] [ text message.senderName ]
        , div [ A.class "date" ] [ text message.receivedAt ]
        , div [ A.class "subject" ] [ text message.subject ]
        , div [ A.class "snippet" ] [ text message.snippet ]
        , div [ A.class "chips" ]
            ((if message.starred then
                [ span [ A.class "chip" ] [ text "Starred" ] ]

              else
                []
             )
                ++ List.map viewLabel message.labels
            )
        ]


viewLabel : String -> Html Msg
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


viewDetailPane : Model -> Html Msg
viewDetailPane model =
    div [ A.class "detail-pane" ]
        [ div [ A.class "detail-toolbar" ]
            [ div [ A.classList [ ( "status", True ), ( "error", model.error /= Nothing ) ] ]
                [ text
                    (case model.error of
                        Just error ->
                            error

                        Nothing ->
                            if model.loading && model.booted then
                                "Working..."

                            else
                                model.syncStatus.state ++ " - " ++ model.syncStatus.lastChecked
                    )
                ]
            , div [ A.class "detail-actions" ]
                [ button [ A.class "icon-button", A.title "Mark read or unread", Ev.onClick ToggleSelectedRead ] [ icon "check" ]
                , button [ A.class "icon-button", A.title "Archive", Ev.onClick ArchiveSelected ] [ icon "archive" ]
                , button [ A.class "text-button", Ev.onClick OpenCompose ] [ icon "edit", span [] [ text "Compose" ] ]
                ]
            ]
        , div [ A.class "detail-scroll" ]
            [ case model.selectedMessage of
                Nothing ->
                    div [ A.class "empty-state" ]
                        [ div []
                            [ h2 [] [ text "Select a message" ]
                            , p [] [ text "Message content will appear here." ]
                            ]
                        ]

                Just message ->
                    viewMessageDetail message
            ]
        ]


viewMessageDetail : MessageDetail -> Html Msg
viewMessageDetail message =
    div []
        [ div [ A.class "message-header" ]
            [ h1 [ A.class "message-subject" ] [ text message.subject ]
            , div [ A.class "message-meta" ]
                ([ span [] [ text "From" ]
                 , strong [] [ text (message.senderName ++ " <" ++ message.senderEmail ++ ">") ]
                 , span [] [ text "To" ]
                 , span [] [ text (String.join ", " message.to) ]
                 , span [] [ text "Date" ]
                 , span [] [ text message.receivedAt ]
                 ]
                    ++ (if List.isEmpty message.cc then
                            []

                        else
                            [ span [] [ text "Cc" ]
                            , span [] [ text (String.join ", " message.cc) ]
                            ]
                       )
                )
            , div [ A.class "chips" ] (List.map viewLabel message.labels)
            ]
        , div [ A.class "body" ] (List.map (\paragraph -> p [] [ text paragraph ]) message.bodyParagraphs)
        ]


viewComposer : Model -> Html Msg
viewComposer model =
    div [ A.class "composer-backdrop" ]
        [ div [ A.class "composer" ]
            [ div [ A.class "composer-head" ]
                [ span [] [ text "New Message" ]
                , button [ A.class "icon-button", A.title "Close", Ev.onClick CloseCompose ] [ icon "x" ]
                ]
            , div [ A.class "compose-field" ]
                [ label [] [ text "To" ]
                , input [ A.value model.composeTo, Ev.onInput ComposeTo ] []
                ]
            , div [ A.class "compose-field" ]
                [ label [] [ text "Subject" ]
                , input [ A.value model.composeSubject, Ev.onInput ComposeSubject ] []
                ]
            , textarea
                [ A.class "compose-body"
                , A.value model.composeBody
                , Ev.onInput ComposeBody
                ]
                []
            , div [ A.class "composer-foot" ]
                [ button [ A.class "icon-button", A.title "Discard", Ev.onClick CloseCompose ] [ icon "trash" ]
                , button [ A.class "text-button", Ev.onClick SendCompose ] [ icon "send", span [] [ text "Send" ] ]
                ]
            ]
        ]


currentFolderName : Model -> String
currentFolderName model =
    case model.selectedFolderId of
        Just "search" ->
            "Search"

        Just folderId ->
            model.folders
                |> List.filter (\folder -> folder.id == folderId)
                |> List.head
                |> Maybe.map .name
                |> Maybe.withDefault "Mailbox"

        Nothing ->
            "Mailbox"


folderMeta : Model -> String
folderMeta model =
    let
        total =
            List.length model.messages

        unread =
            model.messages |> List.filter .unread |> List.length
    in
    String.fromInt total
        ++ (if total == 1 then
                " message"

            else
                " messages"
           )
        ++ " - "
        ++ String.fromInt unread
        ++ " unread"



-- DECODERS


required : String -> Decoder a -> Decoder (a -> b) -> Decoder b
required name decoder accumulated =
    Decode.map2 (\func value -> func value) accumulated (Decode.field name decoder)


bridgeResponseDecoder : Decoder BridgeResponse
bridgeResponseDecoder =
    Decode.succeed BridgeResponse
        |> required "requestId" Decode.int
        |> required "command" Decode.string
        |> required "ok" Decode.bool
        |> required "data" (Decode.maybe Decode.value)
        |> required "error" (Decode.maybe Decode.string)


accountDecoder : Decoder Account
accountDecoder =
    Decode.succeed Account
        |> required "id" Decode.string
        |> required "displayName" Decode.string
        |> required "emailAddress" Decode.string
        |> required "provider" Decode.string
        |> required "accent" Decode.string


folderDecoder : Decoder Folder
folderDecoder =
    Decode.succeed Folder
        |> required "id" Decode.string
        |> required "accountId" Decode.string
        |> required "name" Decode.string
        |> required "role" Decode.string
        |> required "unreadCount" Decode.int


summaryDecoder : Decoder MessageSummary
summaryDecoder =
    Decode.succeed MessageSummary
        |> required "id" Decode.string
        |> required "accountId" Decode.string
        |> required "folderId" Decode.string
        |> required "senderName" Decode.string
        |> required "senderEmail" Decode.string
        |> required "subject" Decode.string
        |> required "snippet" Decode.string
        |> required "receivedAt" Decode.string
        |> required "unread" Decode.bool
        |> required "starred" Decode.bool
        |> required "labels" (Decode.list Decode.string)


detailDecoder : Decoder MessageDetail
detailDecoder =
    Decode.succeed MessageDetail
        |> required "id" Decode.string
        |> required "accountId" Decode.string
        |> required "folderId" Decode.string
        |> required "senderName" Decode.string
        |> required "senderEmail" Decode.string
        |> required "subject" Decode.string
        |> required "snippet" Decode.string
        |> required "receivedAt" Decode.string
        |> required "unread" Decode.bool
        |> required "starred" Decode.bool
        |> required "labels" (Decode.list Decode.string)
        |> required "to" (Decode.list Decode.string)
        |> required "cc" (Decode.list Decode.string)
        |> required "bodyParagraphs" (Decode.list Decode.string)


syncDecoder : Decoder SyncStatus
syncDecoder =
    Decode.succeed SyncStatus
        |> required "state" Decode.string
        |> required "lastChecked" Decode.string
        |> required "detail" Decode.string


bootstrapDecoder : Decoder BootstrapData
bootstrapDecoder =
    Decode.succeed BootstrapData
        |> required "accounts" (Decode.list accountDecoder)
        |> required "folders" (Decode.list folderDecoder)
        |> required "messages" (Decode.list summaryDecoder)
        |> required "selectedFolderId" Decode.string
        |> required "selectedMessageId" (Decode.maybe Decode.string)
        |> required "selectedMessage" (Decode.maybe detailDecoder)
        |> required "syncStatus" syncDecoder


snapshotDecoder : Decoder MailboxSnapshot
snapshotDecoder =
    Decode.succeed MailboxSnapshot
        |> required "folderId" Decode.string
        |> required "folders" (Decode.list folderDecoder)
        |> required "messages" (Decode.list summaryDecoder)
        |> required "selectedMessageId" (Decode.maybe Decode.string)
        |> required "selectedMessage" (Decode.maybe detailDecoder)
        |> required "syncStatus" syncDecoder


messageUpdateDecoder : Decoder MessageUpdate
messageUpdateDecoder =
    Decode.succeed MessageUpdate
        |> required "folders" (Decode.list folderDecoder)
        |> required "message" detailDecoder
        |> required "syncStatus" syncDecoder



-- EVENTS


onEnter : Msg -> Html.Attribute Msg
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

        "check" ->
            Svg.svg attrs [ Svg.path [ SA.d "M20 6 9 17l-5-5" ] [] ]

        "dot" ->
            Svg.svg attrs [ Svg.circle [ SA.cx "12", SA.cy "12", SA.r "4", SA.fill "currentColor", SA.stroke "none" ] [] ]

        "edit" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M12 20h9" ] []
                , Svg.path [ SA.d "M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" ] []
                ]

        "folder" ->
            Svg.svg attrs [ Svg.path [ SA.d "M3 7h6l2 2h10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" ] [] ]

        "inbox" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M22 12h-6l-2 3h-4l-2-3H2" ] []
                , Svg.path [ SA.d "m5.45 5.11-3.1 6.2A2 2 0 0 0 2.24 13L4 19a2 2 0 0 0 2 1h12a2 2 0 0 0 2-1l1.76-6a2 2 0 0 0-.11-1.69l-3.1-6.2A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z" ] []
                ]

        "mail" ->
            Svg.svg attrs
                [ Svg.rect [ SA.x "3", SA.y "5", SA.width "18", SA.height "14", SA.rx "2" ] []
                , Svg.path [ SA.d "m3 7 9 6 9-6" ] []
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
            Svg.svg attrs [ Svg.path [ SA.d "m12 2 3.1 6.4 6.9 1-5 4.9 1.2 6.8L12 17.8 5.8 21.1 7 14.3 2 9.4l6.9-1Z" ] [] ]

        "trash" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M3 6h18" ] []
                , Svg.path [ SA.d "M8 6V4h8v2" ] []
                , Svg.path [ SA.d "m6 6 1 15h10l1-15" ] []
                ]

        "x" ->
            Svg.svg attrs
                [ Svg.path [ SA.d "M18 6 6 18" ] []
                , Svg.path [ SA.d "m6 6 12 12" ] []
                ]

        _ ->
            Svg.svg attrs [ Svg.circle [ SA.cx "12", SA.cy "12", SA.r "8" ] [] ]
