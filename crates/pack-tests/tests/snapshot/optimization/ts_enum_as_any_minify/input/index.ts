const i18n = {
  get(payload: { id: string; dm: string }) {
    return payload.dm;
  },
};

export enum RefType {
  property = i18n.get({
    id: 'property',
    dm: '属性',
  }) as any,
  event = i18n.get({
    id: 'event',
    dm: '事件',
  }) as any,
}

console.log(RefType.property, RefType.event);
