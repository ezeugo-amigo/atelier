module Main exposing (main)

import Api
import Browser
import Dict
import Html exposing (Html, div)
import Html.Attributes as A
import Json.Decode as Decode
import Json.Encode as Encode
import Task
import Time
import Types exposing (..)
import View.MessageList exposing (viewListPane)
import View.Reading exposing (viewComposer, viewDetailPane)
import View.Common
import View.Setup exposing (setupIsClosed, viewSetupOverlay, viewSetupPage)
import View.Sidebar exposing (viewSidebar)


type alias Flags =
    {}


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
    ( initialModel
    , Cmd.batch
        [ Api.sendCommand 1 "app_bootstrap" Encode.null
        , Task.perform GotTimeZone Time.here
        , Task.perform Tick Time.now
        ]
    )


subscriptions : Model -> Sub Msg
subscriptions _ =
    Sub.batch
        [ Api.commandIn GotCommand
        , Api.eventIn GotEvent

        -- Keeps "Yesterday" honest in a window left open overnight.
        , Time.every 60000 Tick
        ]



-- UPDATE


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        GotCommand value ->
            handleCommand value model

        GotEvent value ->
            handleEvent value model

        GotTimeZone zone ->
            ( { model | timeZone = zone }, Cmd.none )

        Tick now ->
            ( { model | now = now }, Cmd.none )

        SelectFolder folderId ->
            enqueue SelectFolderRequest
                "select_folder"
                (Encode.object [ ( "folderId", Encode.string folderId ) ])
                { model | search = "" }

        SelectMessage messageId ->
            enqueue SelectMessageRequest
                "select_message"
                (Encode.object [ ( "messageId", Encode.string messageId ) ])
                model

        SearchInput value ->
            ( { model | search = value }, Cmd.none )

        RunSearch ->
            enqueue SearchRequest
                "search_messages"
                (Encode.object [ ( "query", Encode.string model.search ) ])
                model

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
                    enqueue ArchiveRequest
                        "archive_message"
                        (Encode.object [ ( "messageId", Encode.string messageId ) ])
                        model

        StartAddAccount ->
            ( { model | setup = ChoosingProvider, error = Nothing }, Cmd.none )

        CancelAddAccount ->
            ( { model
                | setup =
                    if List.isEmpty model.accounts then
                        ChoosingProvider

                    else
                        SetupClosed
                , error = Nothing
              }
            , Cmd.none
            )

        BackToProviders ->
            ( { model | setup = ChoosingProvider, loginEmail = "", error = Nothing }, Cmd.none )

        BeginProviderLogin provider ->
            enqueue BeginLoginRequest
                "begin_account_login"
                (Encode.object [ ( "provider", Encode.string provider ) ])
                { model | setup = StartingLogin provider, syncProgress = Nothing }

        LoginEmailInput value ->
            ( { model | loginEmail = value }, Cmd.none )

        AuthorizeAccount ->
            case model.setup of
                MockLogin login ->
                    if String.trim model.loginEmail == "" then
                        ( { model | error = Just "Enter an email address to continue" }, Cmd.none )

                    else
                        enqueue CompleteLoginRequest
                            "complete_account_login"
                            (Encode.object
                                [ ( "provider", Encode.string login.provider )
                                , ( "loginState", Encode.string login.loginState )
                                , ( "emailAddress", Encode.string model.loginEmail )
                                ]
                            )
                            { model | setup = LoadingMailbox login.provider }

                _ ->
                    ( model, Cmd.none )

        DisconnectAccount accountId ->
            enqueue DisconnectRequest
                "disconnect_account"
                (Encode.object [ ( "accountId", Encode.string accountId ) ])
                model

        ToggleAccountFolders accountId ->
            let
                collapsed =
                    Dict.get accountId model.collapsedAccounts
                        |> Maybe.withDefault False
            in
            ( { model
                | collapsedAccounts =
                    Dict.insert accountId (not collapsed) model.collapsedAccounts
              }
            , Cmd.none
            )

        OpenCompose ->
            ( { model | composeOpen = True, composeState = ComposeIdle }, Cmd.none )

        CloseCompose ->
            ( { model | composeOpen = False, composeState = ComposeIdle }, Cmd.none )

        ComposeTo value ->
            ( { model | composeTo = value }, Cmd.none )

        ComposeSubject value ->
            ( { model | composeSubject = value }, Cmd.none )

        ComposeBody value ->
            ( { model | composeBody = value }, Cmd.none )

        SendCompose ->
            -- Sending lands in Phase 6. Until then the draft stays put and the
            -- composer says what actually happened.
            ( { model | composeState = ComposeSent }, Cmd.none )

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
    , Api.sendCommand requestId command payload
    )



-- EVENTS FROM RUST


{-| The OAuth callback arrives here, not as a command response. Every event that
belongs to a login carries the `loginState` it was started with, and anything
that does not match the flow currently on screen is dropped: a user who abandons
one consent flow and starts another produces two callbacks, and only one of them
is the one being shown.
-}
handleEvent : Encode.Value -> Model -> ( Model, Cmd Msg )
handleEvent value model =
    case Decode.decodeValue Api.eventDecoder value of
        Err _ ->
            -- A malformed event is not worth surfacing to the user.
            ( model, Cmd.none )

        Ok event ->
            case event.kind of
                "sync.progress" ->
                    ( { model | syncProgress = event.progress }, Cmd.none )

                "login.completed" ->
                    if matchesActiveLogin model event.loginState then
                        case event.bootstrap of
                            Just bootstrap ->
                                ( applyBootstrap bootstrap
                                    { model | loading = False, error = Nothing }
                                , Cmd.none
                                )

                            Nothing ->
                                ( model, Cmd.none )

                    else
                        ( model, Cmd.none )

                "login.failed" ->
                    if matchesActiveLogin model event.loginState then
                        ( { model
                            | loading = False
                            , setup =
                                LoginFailed
                                    (Maybe.withDefault "Sign-in did not finish." event.message)
                          }
                        , Cmd.none
                        )

                    else
                        ( model, Cmd.none )

                _ ->
                    ( model, Cmd.none )


matchesActiveLogin : Model -> Maybe String -> Bool
matchesActiveLogin model eventState =
    case ( model.setup, eventState ) of
        ( WaitingForCallback _ expected, Just actual ) ->
            expected == actual

        _ ->
            False



-- COMMAND RESPONSES


handleCommand : Encode.Value -> Model -> ( Model, Cmd Msg )
handleCommand value model =
    case Decode.decodeValue Api.bridgeResponseDecoder value of
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
                        ( recoverFailedRequest kind
                            { nextModel
                                | loading = False
                                , error =
                                    Just
                                        (Maybe.withDefault
                                            ("Command failed: " ++ response.command)
                                            response.error
                                        )
                            }
                        , Cmd.none
                        )


applyResponse : RequestKind -> Api.BridgeResponse -> Model -> ( Model, Cmd Msg )
applyResponse kind response model =
    case response.data of
        Nothing ->
            ( { model
                | loading = False
                , error = Just ("Missing response data for " ++ response.command)
              }
            , Cmd.none
            )

        Just data ->
            case kind of
                BootstrapRequest ->
                    decodeInto Api.bootstrapDecoder applyBootstrap data model

                BeginLoginRequest ->
                    decodeInto Api.accountLoginDecoder applyAccountLogin data model

                CompleteLoginRequest ->
                    decodeInto Api.accountSetupResultDecoder applyAccountSetup data model

                DisconnectRequest ->
                    decodeInto Api.bootstrapDecoder applyBootstrap data model

                RefreshRequest ->
                    decodeInto Api.bootstrapDecoder applyBootstrap data model

                SelectFolderRequest ->
                    decodeInto Api.snapshotDecoder applySnapshot data model

                SearchRequest ->
                    decodeInto Api.snapshotDecoder applySnapshot data model

                ArchiveRequest ->
                    decodeInto Api.snapshotDecoder applySnapshot data model

                SelectMessageRequest ->
                    decodeInto Api.messageUpdateDecoder applyMessageUpdate data model

                MarkReadRequest ->
                    decodeInto Api.messageUpdateDecoder applyMessageUpdate data model


decodeInto :
    Decode.Decoder a
    -> (a -> Model -> Model)
    -> Encode.Value
    -> Model
    -> ( Model, Cmd Msg )
decodeInto decoder apply data model =
    case Decode.decodeValue decoder data of
        Ok decoded ->
            ( apply decoded { model | loading = False, error = Nothing }, Cmd.none )

        Err error ->
            ( { model | loading = False, error = Just (Decode.errorToString error) }, Cmd.none )


recoverFailedRequest : RequestKind -> Model -> Model
recoverFailedRequest kind model =
    case kind of
        BeginLoginRequest ->
            { model | setup = ChoosingProvider }

        CompleteLoginRequest ->
            { model | setup = ChoosingProvider }

        _ ->
            model


applyBootstrap : BootstrapData -> Model -> Model
applyBootstrap data model =
    { model
        | providerOptions = data.providerOptions
        , accounts = data.accounts
        , folders = data.folders
        , messages = data.messages
        , selectedFolderId =
            if data.selectedFolderId == "" then
                Nothing

            else
                Just data.selectedFolderId
        , selectedMessageId = data.selectedMessageId
        , selectedMessage = data.selectedMessage
        , syncStatus = data.syncStatus
        , booted = True
        , setup =
            if List.isEmpty data.accounts then
                ChoosingProvider

            else
                SetupClosed
    }


{-| A browser login returns immediately after the browser opens, so the setup
view moves to a waiting state holding the correlation id. A mock login shows the
typed-email form instead.
-}
applyAccountLogin : AccountLogin -> Model -> Model
applyAccountLogin login model =
    if login.browserLogin then
        { model
            | setup = WaitingForCallback login.provider login.loginState
            , syncStatus =
                { state = "Authorizing"
                , lastChecked = model.syncStatus.lastChecked
                , detail = "Waiting for Google in your browser"
                }
        }

    else
        { model
            | setup = MockLogin login
            , loginEmail = View.Common.suggestedEmail login.provider
            , syncStatus =
                { state = "Authorizing"
                , lastChecked = model.syncStatus.lastChecked
                , detail = "Mock OAuth session opened"
                }
        }


applyAccountSetup : AccountSetupResult -> Model -> Model
applyAccountSetup result model =
    let
        bootstrapped =
            applyBootstrap result.bootstrap model
    in
    { bootstrapped
        | credentialPreview = Just result.credential
        , setup = SetupClosed
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



-- VIEW


view : Model -> Html Msg
view model =
    if (not model.booted) || List.isEmpty model.accounts then
        viewSetupPage model

    else if setupIsClosed model.setup then
        viewAppShell model []

    else
        viewAppShell model [ viewSetupOverlay model ]


viewAppShell : Model -> List (Html Msg) -> Html Msg
viewAppShell model overlays =
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
            ++ overlays
        )
