# NPM Publishing Error Fix

## Problem

The TypeScript SDK publishing is failing with the following error:

```
npm error 404 Not Found - PUT https://registry.npmjs.org/@wholelottahoopla%2fprompt-backend-client
npm error 404  '@wholelottahoopla/prompt-backend-client@0.1.9' is not in this registry.
```

## Root Cause

The npm scope `@wholelottahoopla` **does not exist** in the npm registry. When publishing a scoped package for the first time, the scope must either:

1. Match an existing npm user account name (e.g., `@r33drichards`)
2. Be an npm organization that the publisher has access to

## Solutions

### Option 1: Use GitHub Username Scope (Recommended)

Change the package scope to match the GitHub repository owner's npm username.

**Steps:**

1. Verify that an npm account exists for `r33drichards` at https://www.npmjs.com/~r33drichards
2. Update `sdk/package.json`:
   ```json
   {
     "name": "@wholelottahoopla/prompt-backend-client",
     "version": "0.1.9",
     ...
   }
   ```

3. Update references in documentation files:
   - `README.md`
   - `CLAUDE.md`
   - `sdk/README.md`
   - `.github/workflows/publish-sdk.yml`

4. Update the Nix build script to use the new package name

### Option 2: Create the @wholelottahoopla Scope

If you want to keep the `@wholelottahoopla` scope:

1. **Create an npm organization** named `wholelottahoopla`:
   - Go to https://www.npmjs.com/org/create
   - Enter organization name: `wholelottahoopla`
   - Choose plan (free or paid)
   
2. **Or create a personal npm account** with username `wholelottahoopla`:
   - Go to https://www.npmjs.com/signup
   - Create account with username `wholelottahoopla`

3. **Grant publishing access**:
   - Add the npm account that owns `NPM_TOKEN` to the organization
   - Or update `NPM_TOKEN` secret with the new account's token

### Option 3: Use Unscoped Package Name

Publish without a scope (not recommended for organizational packages):

```json
{
  "name": "prompt-backend-client",
  "version": "0.1.9",
  ...
}
```

**Note:** Unscoped package names must be globally unique and are harder to manage for organizations.

## Recommended Action

**Use Option 1** - Change to `@wholelottahoopla/prompt-backend-client` because:
- ✅ Aligns with the GitHub repository owner
- ✅ No additional npm setup required (if account exists)
- ✅ Clear ownership and namespace
- ✅ Follows npm best practices for scoped packages

## Implementation

See the pull request that implements Option 1 with all necessary file updates.

## Verification

After implementing the fix, verify:

1. The npm account exists and has publishing permissions
2. The `NPM_TOKEN` secret is set in GitHub repository settings
3. The token has permission to publish packages under the chosen scope
4. Test the workflow with a manual trigger first

## Additional Resources

- [npm Scopes Documentation](https://docs.npmjs.com/about-scopes)
- [Creating Organizations on npm](https://docs.npmjs.com/creating-an-organization)
- [Publishing Scoped Packages](https://docs.npmjs.com/creating-and-publishing-scoped-public-packages)
