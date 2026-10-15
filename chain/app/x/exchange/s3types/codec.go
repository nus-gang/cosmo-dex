package s3types

import (
	codectypes "github.com/cosmos/cosmos-sdk/codec/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/msgservice"
)

func RegisterInterfaces(r codectypes.InterfaceRegistry) {
	r.RegisterImplementations((*sdk.Msg)(nil), &MsgSettleBatch{}, &MsgCloseBatch{}, &MsgRotateSettlementOperator{}, &MsgRevokeOrder{}, &MsgBumpOrderEpoch{})
	msgservice.RegisterMsgServiceDesc(r, &_Msg_serviceDesc)
}
