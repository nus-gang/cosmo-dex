# 교차 component 출처 재현

검토자는 `source_reconciliation.py`로 고정 commit의 원문/mode를 재현 병합과 대조한다. 작업 트리 변경은 검사하지 않으며 승인·manifest 봉인 예외·runtime 허가를 발급하지 않는다.

```sh
python3 ops/s3-local/source_reconciliation.py \
  --source /absolute/path/to/checkout \
  --left 20c0cd9af0eb305341ee5c7e058350389f03a2b3 \
  --right 46546d317701b127da8196e9e6abdab7ce9a3d6e \
  --candidate EXACT_40_HEX_COMMIT \
  --prefix exchange/ \
  --scratch "$PAPERCLIP_RUN_SCRATCH_DIR" > reconciliation.json
```

종료 코드 0은 exact 재현 일치, 1은 불일치/충돌이며 둘 다 JSON 근거를 출력한다. 2는 입력/실행 오류이며 보고서 stdout은 비어 있다. `HEAD` 같은 ref, 중복/축약 옵션을 받지 않는다. scratch는 이미 존재하는 절대 디렉터리를 지정한다. 임시 병합 파일은 종료 시 제거한다. Git 저장소/작업 파일은 수정하지 않는다. 보고서의 부모/tree/후보/SHA와 실제 독립 심사 출처는 최종 후보 심사에서 별도로 결합한다.

초기 단일 component 검사와 이 결과를 함께 보존한다. 이 명령의 성공으로 `COMPONENT_SOURCE_RECONCILIATION_REQUIRED`를 우회하지 않는다. 서비스/START/RPC 실행 경로는 없다.
