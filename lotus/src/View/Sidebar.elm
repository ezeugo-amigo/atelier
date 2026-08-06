module View.Sidebar exposing (viewSidebar)

import Dict
import Html exposing (Html, button, div, span, text)
import Html.Attributes as A
import Html.Events as Ev
import Time
import Types exposing (..)
import View.Common exposing (formatReceivedAt, icon, iconForRole, viewUnreadCount)


viewSidebar : Model -> Html Msg
viewSidebar model =
    div [ A.class "sidebar" ]
        [ div [ A.class "brand" ]
            [ div [ A.class "brand-title" ]
                [ span [ A.class "brand-mark" ] [ text "L" ]
                , span [] [ text "lotus" ]
                ]
            , div [ A.class "sidebar-actions" ]
                [ button [ A.class "icon-button", A.title "Refresh", Ev.onClick Refresh ]
                    [ icon "refresh" ]
                , button [ A.class "icon-button", A.title "Compose", Ev.onClick OpenCompose ]
                    [ icon "edit" ]
                ]
            ]
        , div [ A.class "nav-section" ]
            [ div [ A.class "nav-heading-row" ]
                [ div [ A.class "nav-heading" ] [ text "Accounts" ]
                , button
                    [ A.class "icon-button", A.title "Add account", Ev.onClick StartAddAccount ]
                    [ icon "plus" ]
                ]
            , viewUnifiedInbox model
            , div [ A.class "account-groups" ] (List.map (viewAccountGroup model) model.accounts)
            ]
        , viewSyncStrip model
        ]


viewSyncStrip : Model -> Html Msg
viewSyncStrip model =
    div [ A.class "sync-strip" ]
        [ div [ A.class "sync-line" ]
            [ span [ A.class "sync-state" ] [ text model.syncStatus.state ]
            , span [] [ text (lastCheckedLabel model) ]
            ]
        , div []
            [ text
                (case model.syncProgress of
                    Just progress ->
                        progress.detail

                    Nothing ->
                        model.syncStatus.detail
                )
            ]
        ]


lastCheckedLabel : Model -> String
lastCheckedLabel model =
    case model.syncStatus.lastChecked of
        Nothing ->
            "Not synced"

        Just checked ->
            formatReceivedAt model.timeZone model.now checked


viewUnifiedInbox : Model -> Html Msg
viewUnifiedInbox model =
    button
        [ A.classList
            [ ( "folder-row", True )
            , ( "unified-row", True )
            , ( "active", model.selectedFolderId == Just unifiedInboxId )
            ]
        , Ev.onClick (SelectFolder unifiedInboxId)
        ]
        [ span [] [ icon "inbox" ]
        , span [ A.class "folder-name" ] [ text "Unified Inbox" ]
        , viewUnreadCount (unifiedUnreadCount model)
        ]


unifiedUnreadCount : Model -> Int
unifiedUnreadCount model =
    model.folders
        |> List.filter (\folder -> folder.role == "inbox")
        |> List.map .unreadCount
        |> List.sum


viewAccountGroup : Model -> Account -> Html Msg
viewAccountGroup model account =
    let
        collapsed =
            Dict.get account.id model.collapsedAccounts
                |> Maybe.withDefault False

        accountFolders =
            model.folders
                |> List.filter (\folder -> folder.accountId == account.id)
    in
    div [ A.class "account-group" ]
        [ div [ A.class "account-header-row" ]
            [ button [ A.class "account-header", Ev.onClick (ToggleAccountFolders account.id) ]
                [ span [ A.style "color" account.accent ] [ icon "dot" ]
                , span [ A.class "account-heading" ]
                    [ span [ A.class "account-name" ] [ text account.displayName ]
                    , span [ A.class "account-email" ] [ text account.emailAddress ]
                    ]
                , span [ A.class "account-provider" ]
                    [ text
                        (if account.connected then
                            account.provider

                         else
                            "Disconnected"
                        )
                    ]
                , span [ A.class "collapse-icon" ]
                    [ icon
                        (if collapsed then
                            "chevron-right"

                         else
                            "chevron-down"
                        )
                    ]
                ]
            , if account.connected then
                span [] []

              else
                button
                    [ A.class "icon-button"
                    , A.title "Reconnect"
                    , Ev.onClick (BeginProviderLogin account.providerKind)
                    ]
                    [ icon "refresh" ]
            , button
                [ A.class "icon-button"
                , A.title "Disconnect account"
                , Ev.onClick (DisconnectAccount account.id)
                ]
                [ icon "unlink" ]
            ]
        , if collapsed then
            div [] []

          else
            div [ A.class "account-folders" ]
                (List.map (viewFolder model.selectedFolderId) accountFolders)
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
        , viewUnreadCount folder.unreadCount
        ]
