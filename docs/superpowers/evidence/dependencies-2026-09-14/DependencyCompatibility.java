import com.google.gson.Gson;
import com.google.gson.JsonObject;
import org.bouncycastle.util.io.pem.PemObject;
import org.bouncycastle.util.io.pem.PemReader;
import java.io.StringReader;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.Map;

public final class DependencyCompatibility {
  public static void main(String[] args) throws Exception {
    Gson gson = new Gson();
    Map<String, Object> props = new LinkedHashMap<>();
    props.put("message", "viewer 준비");
    props.put("count", 3);
    JsonObject parsed = gson.fromJson(gson.toJson(props), JsonObject.class);
    if (!parsed.get("message").getAsString().equals("viewer 준비") || parsed.get("count").getAsInt() != 3) {
      throw new AssertionError("Expo log-box JSON API round trip changed");
    }
    // Synthetic bytes only: this probe does not read or create an actual key.
    try (PemReader reader = new PemReader(new StringReader("-----BEGIN PRIVATE KEY-----\nAQID\n-----END PRIVATE KEY-----\n"))) {
      PemObject object = reader.readPemObject();
      if (!object.getType().equals("PRIVATE KEY") || !Arrays.equals(object.getContent(), new byte[]{1, 2, 3})) {
        throw new AssertionError("tcp-socket PEM API changed");
      }
    }
    try (PemReader reader = new PemReader(new StringReader("-----BEGIN PRIVATE KEY-----\nAQID\n"))) {
      reader.readPemObject();
      throw new AssertionError("Truncated PEM should be rejected");
    } catch (java.io.IOException expected) { }
    System.out.println("PASS: Gson JSON round trip; PEM content; truncated PEM rejection (3 checks)");
  }
}
