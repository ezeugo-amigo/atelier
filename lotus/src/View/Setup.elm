module View.Setup exposing (setupIsClosed, viewSetupOverlay, viewSetupPage)

import Html exposing (Html, button, div, h1, input, label, p, span, text)
import Html.Attributes as A
import Html.Events as Ev
import Types exposing (..)
import View.Common exposing (icon, onEnter, providerDisplayName)


setupIsClosed : SetupState -> Bool
setupIsClosed setup =
    case setup of
        SetupClosed ->
            True

        _ ->
            False


viewSetupPage : Model -> Html Msg
viewSetupPage model =
    div [ A.class "setup-page" ]
        [ div [ A.class "setup-panel" ]
            (div [ A.class "setup-brand" ]
                [ span [ A.class "brand-mark setup-mark" ] [ text "L" ]
                , span [] [ text "lotus" ]
                ]
                :: viewSetupContent model
            )
        ]


viewSetupOverlay : Model -> Html Msg
viewSetupOverlay model =
    div [ A.class "setup-overlay" ]
        [ div [ A.class "setup-panel compact" ]
            (button
                [ A.class "icon-button setup-close"
                , A.title "Close"
                , Ev.onClick CancelAddAccount
                ]
                [ icon "x" ]
                :: viewSetupContent model
            )
        ]


viewSetupContent : Model -> List (Html Msg)
viewSetupContent model =
    let
        errorView =
            case model.error of
                Nothing ->
                    []

                Just error ->
                    [ div [ A.class "setup-error" ] [ text error ] ]
    in
    case model.setup of
        SetupClosed ->
            viewProviderChoice model ++ errorView

        ChoosingProvider ->
            viewProviderChoice model ++ errorView

        StartingLogin provider ->
            viewSetupLoading
                ("Opening " ++ providerDisplayName model provider)
                "Preparing an authorization session."
                ++ errorView

        MockLogin login ->
            viewMockLogin model login ++ errorView

        WaitingForCallback provider _ ->
            viewWaitingForCallback model provider ++ errorView

        LoginFailed message ->
            viewLoginFailed message

        LoadingMailbox provider ->
            viewSetupLoading
                ("Loading " ++ providerDisplayName model provider)
                (importDetail model)
                ++ errorView


importDetail : Model -> String
importDetail model =
    case model.syncProgress of
        Just progress ->
            progress.detail

        Nothing ->
            "Storing credentials and importing your inbox."


viewProviderChoice : Model -> List (Html Msg)
viewProviderChoice model =
    [ div [ A.class "setup-heading" ]
        [ h1 [] [ text "Add Account" ]
        , p [] [ text "Choose a provider to connect." ]
        ]
    , div [ A.class "provider-grid" ]
        (if List.isEmpty model.providerOptions then
            [ div [ A.class "setup-muted" ] [ text "Loading providers..." ] ]

         else
            List.map viewProviderOption model.providerOptions
        )
    ]


viewProviderOption : ProviderOption -> Html Msg
viewProviderOption option =
    button
        [ A.classList
            [ ( "provider-card", True )
            , ( "provider-real", option.browserLogin )
            ]
        , Ev.onClick (BeginProviderLogin option.provider)
        ]
        [ span [ A.class "provider-icon" ]
            [ icon
                (if option.browserLogin then
                    "globe"

                 else
                    "mail"
                )
            ]
        , span [ A.class "provider-name" ] [ text option.displayName ]
        , span [ A.class "provider-description" ] [ text option.description ]
        ]


{-| The browser flow asks for nothing. No email input: the address comes from
Gmail's own profile response after consent.
-}
viewWaitingForCallback : Model -> String -> List (Html Msg)
viewWaitingForCallback model provider =
    [ div [ A.class "setup-heading" ]
        [ h1 [] [ text ("Waiting for " ++ providerDisplayName model provider) ]
        , p [] [ text "Finish signing in with Google in your browser, then come back here." ]
        ]
    , div [ A.class "setup-loader" ]
        [ span [ A.class "spinner" ] []
        , span [] [ text "Waiting for authorization" ]
        ]
    , div [ A.class "setup-actions" ]
        [ button [ A.class "icon-button", A.title "Back", Ev.onClick BackToProviders ]
            [ icon "arrow-left" ]
        ]
    ]


viewLoginFailed : String -> List (Html Msg)
viewLoginFailed message =
    [ div [ A.class "setup-heading" ]
        [ h1 [] [ text "Sign-in did not finish" ]
        , p [] [ text message ]
        ]
    , div [ A.class "setup-actions" ]
        [ button [ A.class "text-button", Ev.onClick BackToProviders ]
            [ icon "arrow-left", span [] [ text "Try again" ] ]
        ]
    ]


{-| Mock providers keep the typed-email form: they have no real OAuth flow.
-}
viewMockLogin : Model -> AccountLogin -> List (Html Msg)
viewMockLogin model login =
    [ div [ A.class "setup-heading" ]
        [ h1 [] [ text ("Sign in to " ++ providerDisplayName model login.provider) ]
        , p [] [ text "Authorize Lotus to read mail and refresh access while offline." ]
        ]
    , div [ A.class "mock-login-url" ] [ text login.loginUrl ]
    , div [ A.class "compose-field setup-field" ]
        [ label [] [ text "Email" ]
        , input
            [ A.value model.loginEmail
            , Ev.onInput LoginEmailInput
            , onEnter AuthorizeAccount
            ]
            []
        ]
    , div [ A.class "scope-list" ] (List.map viewScope login.scopes)
    , div [ A.class "setup-actions" ]
        [ button [ A.class "icon-button", A.title "Back", Ev.onClick BackToProviders ]
            [ icon "arrow-left" ]
        , if List.isEmpty model.accounts then
            span [] []

          else
            button [ A.class "icon-button", A.title "Cancel", Ev.onClick CancelAddAccount ]
                [ icon "x" ]
        , button [ A.class "text-button", Ev.onClick AuthorizeAccount ]
            [ icon "key", span [] [ text "Authorize" ] ]
        ]
    ]


viewScope : String -> Html Msg
viewScope scope =
    span [ A.class "scope-chip" ] [ icon "check", text (shortScope scope) ]


{-| A full Google scope URL overflows the chip. Show the trailing segment.
-}
shortScope : String -> String
shortScope scope =
    if String.startsWith "https://" scope then
        String.split "/" scope |> List.reverse |> List.head |> Maybe.withDefault scope

    else
        scope


viewSetupLoading : String -> String -> List (Html Msg)
viewSetupLoading title detail =
    [ div [ A.class "setup-heading" ]
        [ h1 [] [ text title ]
        , p [] [ text detail ]
        ]
    , div [ A.class "setup-loader" ]
        [ span [ A.class "spinner" ] []
        , span [] [ text "Please wait" ]
        ]
    ]
