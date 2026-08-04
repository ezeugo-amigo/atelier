module View.Reading exposing (viewComposer, viewDetailPane)

import Html exposing (Html, button, div, h1, h2, input, label, p, span, strong, text, textarea)
import Html.Attributes as A
import Html.Events as Ev
import Types exposing (..)
import View.Common exposing (formatReceivedAt, icon, viewLabel)


viewDetailPane : Model -> Html Msg
viewDetailPane model =
    div [ A.class "detail-pane" ]
        [ div [ A.class "detail-toolbar" ]
            [ div [ A.classList [ ( "status", True ), ( "error", model.error /= Nothing ) ] ]
                [ text (statusLine model) ]
            , div [ A.class "detail-actions" ]
                [ button
                    [ A.class "icon-button"
                    , A.title "Mark read or unread"
                    , Ev.onClick ToggleSelectedRead
                    ]
                    [ icon "check" ]
                , button [ A.class "icon-button", A.title "Archive", Ev.onClick ArchiveSelected ]
                    [ icon "archive" ]
                , button [ A.class "text-button", Ev.onClick OpenCompose ]
                    [ icon "edit", span [] [ text "Compose" ] ]
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
                    viewMessageDetail model message
            ]
        ]


statusLine : Model -> String
statusLine model =
    case model.error of
        Just error ->
            error

        Nothing ->
            if model.loading && model.booted then
                "Working..."

            else
                case model.syncStatus.lastChecked of
                    Nothing ->
                        model.syncStatus.state

                    Just checked ->
                        model.syncStatus.state
                            ++ " - "
                            ++ formatReceivedAt model.timeZone model.now checked


viewMessageDetail : Model -> MessageDetail -> Html Msg
viewMessageDetail model message =
    div []
        [ div [ A.class "message-header" ]
            [ h1 [ A.class "message-subject" ] [ text message.subject ]
            , div [ A.class "message-meta" ]
                ([ span [] [ text "From" ]
                 , strong [] [ text (senderLine message) ]
                 , span [] [ text "To" ]
                 , span [] [ text (String.join ", " message.to) ]
                 , span [] [ text "Date" ]
                 , span []
                    [ text (formatReceivedAt model.timeZone model.now message.receivedAt) ]
                 ]
                    ++ optionalRow "Cc" message.cc
                    ++ optionalRow "Reply-To" message.replyTo
                )
            , div [ A.class "chips" ] (List.map viewLabel message.labels)
            ]
        , div [ A.class "body" ] (viewBody message)
        ]


{-| Real mail can arrive with an empty body: an automated notice with only
headers, or a part shape v1 does not render. Say so rather than showing nothing.
-}
viewBody : MessageDetail -> List (Html Msg)
viewBody message =
    if List.isEmpty message.bodyParagraphs then
        [ p [ A.class "body-empty" ] [ text "This message has no plain-text body." ] ]

    else
        List.map (\paragraph -> p [] [ text paragraph ]) message.bodyParagraphs


senderLine : MessageDetail -> String
senderLine message =
    if message.senderName == "" then
        message.senderEmail

    else
        message.senderName ++ " <" ++ message.senderEmail ++ ">"


optionalRow : String -> List String -> List (Html Msg)
optionalRow name values =
    if List.isEmpty values then
        []

    else
        [ span [] [ text name ]
        , span [] [ text (String.join ", " values) ]
        ]


viewComposer : Model -> Html Msg
viewComposer model =
    div [ A.class "composer-backdrop" ]
        [ div [ A.class "composer" ]
            [ div [ A.class "composer-head" ]
                [ span [] [ text "New Message" ]
                , button [ A.class "icon-button", A.title "Close", Ev.onClick CloseCompose ]
                    [ icon "x" ]
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
            , viewComposeStatus model.composeState
            , div [ A.class "composer-foot" ]
                [ button [ A.class "icon-button", A.title "Discard", Ev.onClick CloseCompose ]
                    [ icon "trash" ]
                , button
                    [ A.class "text-button"
                    , A.disabled (model.composeState == ComposeSending)
                    , Ev.onClick SendCompose
                    ]
                    [ icon "send"
                    , span []
                        [ text
                            (case model.composeState of
                                ComposeSending ->
                                    "Sending..."

                                _ ->
                                    "Send"
                            )
                        ]
                    ]
                ]
            ]
        ]


{-| Sending is not implemented yet (Phase 6), so the composer stages a draft and
says so plainly instead of implying the message left.
-}
viewComposeStatus : ComposeState -> Html Msg
viewComposeStatus state =
    case state of
        ComposeIdle ->
            span [] []

        ComposeSending ->
            div [ A.class "compose-status" ] [ text "Sending..." ]

        ComposeSent ->
            div [ A.class "compose-status" ]
                [ text "Saved locally. Sending mail arrives in a later release." ]

        ComposeFailed message ->
            div [ A.class "compose-status error" ] [ text message ]
