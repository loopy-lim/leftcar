export function variantKey(schema) {
    if (typeof schema.const === 'string')
        return schema.const;
    if (Array.isArray(schema.enum) && schema.enum.length === 1)
        return String(schema.enum[0]);
    const properties = schema.properties;
    if (properties) {
        const discriminator = Object.entries(properties).find(([, value]) => value.const !== undefined);
        if (discriminator)
            return String(discriminator[1].const);
        const keys = Object.keys(properties);
        if (keys.length === 1)
            return keys[0];
    }
    return typeof schema.title === 'string' ? schema.title : null;
}
export function discriminator(schema) {
    for (const [key, value] of Object.entries(schema.properties ?? {})) {
        if (value.const !== undefined)
            return { key, value: value.const };
    }
    return null;
}
//# sourceMappingURL=complex-codec-variants.js.map