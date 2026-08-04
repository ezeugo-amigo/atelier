module View.MessageList exposing (viewListPane)

import Html exposing (Html, button, div, h1, h2, input, p, span, text)
import Html.Attributes as A
import Html.Events as Ev
import Types exposing (..)
import View.Common exposing (formatReceivedAt, icon, onEnter, viewLabel)


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
            , button [ A.class "icon-button", A.title "Run search", Ev.onClick RunSearch ]
                [ icon "arrow-right" ]
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
                List.map (viewMessageRow model) model.messages
            )
        ]


viewMessageRow : Model -> MessageSummary -> Html Msg
viewMessageRow model message =
    button
        [ A.classList
            [ ( "message-row", True )
            , ( "unread", message.unread )
            , ( "selected", model.selectedMessageId == Just message.id )
            ]
        , Ev.onClick (SelectMessage message.id)
        ]
        [ div [ A.class "sender" ] [ text message.senderName ]
        , div [ A.class "date" ]
            [ text (formatReceivedAt model.timeZone model.now message.receivedAt) ]
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


currentFolderName : Model -> String
currentFolderName model =
    case model.selectedFolderId of
        Just "search" ->
            "Search"

        Just folderId ->
            if folderId == unifiedInboxId then
                "Unified Inbox"

            else
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
