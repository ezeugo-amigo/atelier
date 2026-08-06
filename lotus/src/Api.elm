port module Api exposing
    ( BridgeResponse
    , accountLoginDecoder
    , accountSetupResultDecoder
    , bootstrapDecoder
    , bridgeResponseDecoder
    , commandIn
    , eventIn
    , eventDecoder
    , iso8601Decoder
    , messageUpdateDecoder
    , parseIso8601
    , sendCommand
    , snapshotDecoder
    )

import Json.Decode as Decode exposing (Decoder)
import Json.Encode as Encode
import Time
import Types exposing (..)



-- PORTS


port commandOut : Encode.Value -> Cmd msg


port commandIn : (Encode.Value -> msg) -> Sub msg


{-| Rust pushes here. The OAuth callback lands on a loopback socket, so it cannot
come back as a command response.
-}
port eventIn : (Encode.Value -> msg) -> Sub msg


sendCommand : Int -> String -> Encode.Value -> Cmd msg
sendCommand requestId command payload =
    commandOut
        (Encode.object
            [ ( "requestId", Encode.int requestId )
            , ( "command", Encode.string command )
            , ( "payload", payload )
            ]
        )



-- TIME


{-| Parse the ISO-8601 UTC strings Rust puts on the wire. Elm has no date parser
in core, and pulling one in for a single fixed format is not worth it: the
producer is our own serializer, so the shape is exactly
`YYYY-MM-DDTHH:MM:SS(.fff)?Z`.
-}
parseIso8601 : String -> Maybe Time.Posix
parseIso8601 raw =
    let
        digits from length =
            String.slice from (from + length) raw |> String.toInt
    in
    case ( digits 0 4, digits 5 2, digits 8 2 ) of
        ( Just year, Just month, Just day ) ->
            case ( digits 11 2, digits 14 2, digits 17 2 ) of
                ( Just hour, Just minute, Just second ) ->
                    Just
                        (Time.millisToPosix
                            ((daysFromCivil year month day * 86400 + hour * 3600 + minute * 60 + second)
                                * 1000
                            )
                        )

                _ ->
                    -- A date with no time component still sorts and renders.
                    Just (Time.millisToPosix (daysFromCivil year month day * 86400000))

        _ ->
            Nothing


{-| Howard Hinnant's days_from_civil. Converts a proleptic Gregorian date to a
day count from 1970-01-01, handling leap years without a lookup table.
-}
daysFromCivil : Int -> Int -> Int -> Int
daysFromCivil year month day =
    let
        shiftedYear =
            if month <= 2 then
                year - 1

            else
                year

        era =
            (if shiftedYear >= 0 then
                shiftedYear

             else
                shiftedYear - 399
            )
                // 400

        yearOfEra =
            shiftedYear - era * 400

        shiftedMonth =
            if month > 2 then
                month - 3

            else
                month + 9

        dayOfYear =
            (153 * shiftedMonth + 2) // 5 + day - 1

        dayOfEra =
            yearOfEra * 365 + yearOfEra // 4 - yearOfEra // 100 + dayOfYear
    in
    era * 146097 + dayOfEra - 719468


iso8601Decoder : Decoder Time.Posix
iso8601Decoder =
    Decode.string
        |> Decode.andThen
            (\raw ->
                case parseIso8601 raw of
                    Just posix ->
                        Decode.succeed posix

                    Nothing ->
                        Decode.fail ("Not an ISO-8601 timestamp: " ++ raw)
            )



-- DECODERS


required : String -> Decoder a -> Decoder (a -> b) -> Decoder b
required name decoder accumulated =
    Decode.map2 (\func value -> func value) accumulated (Decode.field name decoder)


type alias BridgeResponse =
    { requestId : Int
    , command : String
    , ok : Bool
    , data : Maybe Encode.Value
    , error : Maybe String
    }


bridgeResponseDecoder : Decoder BridgeResponse
bridgeResponseDecoder =
    Decode.succeed BridgeResponse
        |> required "requestId" Decode.int
        |> required "command" Decode.string
        |> required "ok" Decode.bool
        |> required "data" (Decode.maybe Decode.value)
        |> required "error" (Decode.maybe Decode.string)


providerOptionDecoder : Decoder ProviderOption
providerOptionDecoder =
    Decode.succeed ProviderOption
        |> required "provider" Decode.string
        |> required "displayName" Decode.string
        |> required "description" Decode.string
        |> required "browserLogin" Decode.bool


accountLoginDecoder : Decoder AccountLogin
accountLoginDecoder =
    Decode.succeed AccountLogin
        |> required "provider" Decode.string
        |> required "loginUrl" Decode.string
        |> required "loginState" Decode.string
        |> required "expiresAt" Decode.string
        |> required "scopes" (Decode.list Decode.string)
        |> required "browserLogin" Decode.bool


credentialPreviewDecoder : Decoder CredentialPreview
credentialPreviewDecoder =
    Decode.succeed CredentialPreview
        |> required "accountId" Decode.string
        |> required "provider" Decode.string
        |> required "accessTokenTail" Decode.string
        |> required "refreshTokenTail" Decode.string
        |> required "expiresAt" Decode.string


accountDecoder : Decoder Account
accountDecoder =
    Decode.succeed Account
        |> required "id" Decode.string
        |> required "displayName" Decode.string
        |> required "emailAddress" Decode.string
        |> required "provider" Decode.string
        |> required "providerKind" Decode.string
        |> required "accent" Decode.string
        |> required "connected" Decode.bool


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
        |> required "folderIds" (Decode.list Decode.string)
        |> required "senderName" Decode.string
        |> required "senderEmail" Decode.string
        |> required "subject" Decode.string
        |> required "snippet" Decode.string
        |> required "receivedAt" iso8601Decoder
        |> required "unread" Decode.bool
        |> required "starred" Decode.bool
        |> required "labels" (Decode.list Decode.string)


detailDecoder : Decoder MessageDetail
detailDecoder =
    Decode.succeed MessageDetail
        |> required "id" Decode.string
        |> required "accountId" Decode.string
        |> required "folderIds" (Decode.list Decode.string)
        |> required "senderName" Decode.string
        |> required "senderEmail" Decode.string
        |> required "subject" Decode.string
        |> required "snippet" Decode.string
        |> required "receivedAt" iso8601Decoder
        |> required "unread" Decode.bool
        |> required "starred" Decode.bool
        |> required "labels" (Decode.list Decode.string)
        |> required "to" (Decode.list Decode.string)
        |> required "cc" (Decode.list Decode.string)
        |> required "replyTo" (Decode.list Decode.string)
        |> required "bodyParagraphs" (Decode.list Decode.string)


syncDecoder : Decoder SyncStatus
syncDecoder =
    Decode.succeed SyncStatus
        |> required "state" Decode.string
        |> required "lastChecked" (Decode.nullable iso8601Decoder)
        |> required "detail" Decode.string


bootstrapDecoder : Decoder BootstrapData
bootstrapDecoder =
    Decode.succeed BootstrapData
        |> required "providerOptions" (Decode.list providerOptionDecoder)
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


accountSetupResultDecoder : Decoder AccountSetupResult
accountSetupResultDecoder =
    Decode.succeed AccountSetupResult
        |> required "bootstrap" bootstrapDecoder
        |> required "credential" credentialPreviewDecoder


syncProgressDecoder : Decoder SyncProgress
syncProgressDecoder =
    Decode.succeed SyncProgress
        |> required "imported" Decode.int
        |> required "total" (Decode.nullable Decode.int)
        |> required "detail" Decode.string


eventDecoder : Decoder LotusEvent
eventDecoder =
    Decode.succeed LotusEvent
        |> required "kind" Decode.string
        |> required "loginState" (Decode.nullable Decode.string)
        |> required "message" (Decode.nullable Decode.string)
        |> required "progress" (Decode.nullable syncProgressDecoder)
        |> required "bootstrap" (Decode.nullable bootstrapDecoder)
