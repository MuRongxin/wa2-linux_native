import os, sys, json
from volcengine.imagex.v2.imagex_service import ImagexService


def main():
    AK = os.environ.get('VOLC_ACCESSKEY_ID')
    SK = os.environ.get('VOLC_SECRET_ACCESS_KEY')
    SID = os.environ.get('VOLC_IMAGEX_SERVICE_ID')
    if not (AK and SK and SID):
        sys.exit('缺少环境变量 VOLC_ACCESSKEY_ID / VOLC_SECRET_ACCESS_KEY / VOLC_IMAGEX_SERVICE_ID')
    svc = ImagexService()
    svc.set_ak(AK)
    svc.set_sk(SK)
    if len(sys.argv) < 2:
        sys.exit('用法: python3 tools/volc_sr.py <图片> [倍数]')
    path = sys.argv[1]
    mult = int(sys.argv[2]) if len(sys.argv) > 2 else 2
    resp = svc.upload_image(params={'ServiceId': SID}, file_paths=[path])
    print('UPLOAD', json.dumps(resp)[:800])
    res = resp.get('Result', {})
    uri = None
    if 'Results' in res and res['Results']:
        uri = res['Results'][0].get('Uri') or res['Results'][0].get('StoreUri')
    print('URI', uri)
    if not uri:
        sys.exit('未拿到上传 URI')
    wp = json.dumps({'Input': {'ObjectKey': uri, 'DataType': 'uri'}, 'SrParam': {'Mode': 0, 'Multiple': mult, 'ShortMin': 16, 'ShortMax': 1440, 'LongMin': 16, 'LongMax': 2160, 'Policy': 0, 'SharpRatio': 1.0, 'DenoiseRatio': 0.7}})
    body = {'ServiceId': SID, 'WorkflowTemplateId': 'system_workflow_sr', 'WorkflowParameter': wp}
    out = svc.ai_process(query={}, body=body)
    print('SR', json.dumps(out)[:800])


if __name__ == '__main__':
    try:
        main()
    except Exception as e:
        print('ERR', repr(e))
        raise
