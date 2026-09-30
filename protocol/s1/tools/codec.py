"""S1 fixture encoder only. Not an SDK decoder or application implementation."""
import hashlib

def varint(n):
    out=bytearray()
    while n>127: out.append((n&127)|128); n >>= 7
    out.append(n)
    return bytes(out)

def b(tag,value):
    if isinstance(value,str): value=value.encode()
    return varint(tag*8+2)+varint(len(value))+value

def u(tag,value):
    return varint(tag*8)+varint(value) if value else b''

def address(raw):
    chars='qpzry9x8gf2tvdw0s3jn54khce6mua7l'
    h='nus'; data=[]; acc=bits=0
    for x in raw:
        acc=(acc<<8)|x; bits+=8
        while bits>=5: bits-=5; data.append((acc>>bits)&31)
    if bits: data.append((acc<<(5-bits))&31)
    chk=1
    for x in [ord(c)>>5 for c in h]+[0]+[ord(c)&31 for c in h]+data+[0]*6:
        top=chk>>25; chk=((chk&0x1ffffff)<<5)^x
        for i,g in enumerate([0x3b6a57b2,0x26508e6d,0x1ea119fa,0x3d4233dd,0x2a1462b3]):
            if (top>>i)&1: chk ^= g
    chk ^= 1
    return h+'1'+''.join(chars[x] for x in data+[(chk>>5*(5-i))&31 for i in range(6)])

def encode(c,pk):
    msg=b(1,c['owner'])+b(2,'DEVQUOTE')+b(3,c['amount_atoms'])+b(4,bytes.fromhex(c['request_id']))+b(5,c['expected_epoch'])+b(6,c['expiry_height'])+b(7,bytes.fromhex(c['genesis_hash']))
    anymsg=b(1,c['type_url'])+b(2,msg)
    body=b(1,anymsg)
    pkany=b(1,'/cosmos.crypto.mldsa65.PubKey')+b(2,b(1,pk))
    signer=b(1,pkany)+b(2,b(1,u(1,1)))+u(3,int(c['sequence']))
    fee=b(1,b(1,'DEVGAS')+b(2,c['fee_atoms']))+u(2,int(c['gas_limit']))
    auth=b(1,signer)+b(2,fee)
    doc=b(1,body)+b(2,auth)+b(3,c['chain_id'])+u(4,int(c['account_number']))
    domain=c['type_url'].encode()
    frame=len(domain).to_bytes(4,'big')+domain+len(msg).to_bytes(8,'big')+msg
    return {'message_hex':msg.hex(),'body_hex':body.hex(),'auth_info_hex':auth.hex(),'sign_doc_hex':doc.hex(),'request_hash':hashlib.sha256(frame).hexdigest()}
